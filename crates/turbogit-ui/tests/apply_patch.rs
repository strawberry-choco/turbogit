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

fn root_id(dir: &Path) -> RootId {
    RootId(dir.to_path_buf().into())
}

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

fn painted_title(title: &str) -> String {
    title.to_uppercase()
}

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

#[test]
fn show_diff_returns_git_verdict_without_writing_and_without_dispatching() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let patch = conflicting_patch(tmp.path());
    let before = std::fs::read(dir.join("first.txt")).unwrap();

    let mut harness = window(&dir, tmp.path(), PatchSource::File);
    harness.state_mut().ui.dlg.apply_patch_path = patch.to_string_lossy().into_owned();
    settle(&mut harness);
    assert_painted(&harness, "FILES THE PATCH NAMES · 2");
    assert_painted(&harness, "first.txt");

    click(&mut harness, "Show Diff");
    settle(&mut harness);

    assert_painted(
        &harness,
        "The patch does not apply cleanly. No file has been changed.",
    );
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

    assert_eq!(
        std::fs::read(dir.join("first.txt")).unwrap(),
        before_first,
        "the aborted state's message is a claim about these bytes"
    );
    assert_eq!(
        std::fs::read(dir.join("second.txt")).unwrap(),
        before_second
    );

    assert_painted(&harness, &painted_title(ApplyOutcome::ABORTED_TITLE));
    assert_painted(&harness, ApplyOutcome::ABORTED_MESSAGE);
    assert_painted(&harness, "ROLLED BACK · 2");
    assert_not_painted(
        &harness,
        &painted_title(ApplyOutcome::ROLLBACK_FAILED_TITLE),
    );

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
    assert_not_painted(&harness, &painted_title(ApplyOutcome::ABORTED_TITLE));
    assert_not_painted(&harness, ApplyOutcome::ABORTED_MESSAGE);
    assert_painted(&harness, "Restore recorded content");
}

#[test]
fn the_failed_rollback_offers_one_action_and_that_action_fixes_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
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

#[test]
fn a_second_refused_retry_keeps_the_state_named_and_records_it_again() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
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

const RECORDED: &[u8] = b"one\ntwo\nthree\n";
const LEFT_BY_APPLY: &[u8] = b"one\nTWO\nthree\n";

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

#[track_caller]
fn assert_still_rescuable(harness: &mut Harness<'static, AppState>, dir: &Path) {
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

#[test]
fn switching_the_patch_route_leaves_the_failed_rollback_resolvable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let mut harness = stranded(&dir, tmp.path());

    click(&mut harness, "From file");
    settle(&mut harness);

    assert_still_rescuable(&mut harness, &dir);
}

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
