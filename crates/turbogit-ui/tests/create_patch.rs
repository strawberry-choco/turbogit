use std::path::{Path, PathBuf};

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::NodeT as _;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use test_support::harness::{
    assert_not_painted, assert_painted, painted_text, settle, shell_harness_over,
};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{
    Commit, PatchEncoding, PatchOrigin, RootId, Signature, SignatureState, VcsSettings,
};
use turbogit_ui::ui::create_patch;
use turbogit_ui::ui::popups::{Action, run_action};

fn root_id(dir: &Path) -> RootId {
    RootId(dir.to_path_buf().into())
}

const DEEP: &str = "src/main/java/App.java";

const UTIL: &str = "src/main/java/Util.java";

fn dirty_repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(dir.join("src/main/java")).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "ada@example.com"]);
    git(&dir, &["config", "user.name", "Ada Lovelace"]);
    pin_eol_lf(&dir);
    git_commit(&dir, "README.md", "readme\n", "seed");
    git_commit(&dir, DEEP, "one\ntwo\nthree\n", "Align the parser output");
    git_commit(&dir, UTIL, "alpha\nbeta\n", "Add the util");
    std::fs::write(dir.join(DEEP), "one\nTWO\nthree\n").unwrap();
    std::fs::write(dir.join(UTIL), "alpha\nBETA\n").unwrap();
    dir
}

fn clean_repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(dir.join("src/main/java")).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "ada@example.com"]);
    git(&dir, &["config", "user.name", "Ada Lovelace"]);
    pin_eol_lf(&dir);
    git_commit(&dir, "README.md", "readme\n", "seed");
    git_commit(&dir, DEEP, "one\ntwo\nthree\n", "Align the parser output");
    dir
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn cached(root: &RootId, id: &str, message: &str) -> Commit {
    Commit {
        id: id.to_owned(),
        parents: vec![],
        author: Signature {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
            time: 1,
        },
        committer: Signature {
            name: "Ada Lovelace".to_owned(),
            email: "ada@example.com".to_owned(),
            time: 1,
        },
        message: format!("{message}\n\na body nobody names"),
        time: 1,
        root: root.clone(),
        signature: SignatureState::Unsigned,
    }
}

fn state_over(dirs: &[PathBuf], parent: &Path) -> AppState {
    AppState::for_roots(parent, dirs).with_settings(VcsSettings::default())
}

fn settled(state: AppState) -> Harness<'static, AppState> {
    let mut harness = shell_harness_over(state, Vec2::new(1280.0, 900.0));
    settle(&mut harness);
    harness
}

fn local_changes(dir: &Path, parent: &Path) -> Harness<'static, AppState> {
    let mut state = state_over(&[dir.to_path_buf()], parent);
    run_action(&mut state, Action::CreatePatch);
    let harness = settled(state);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::CreatePatchComment),
        "local changes have no name of their own, so the comment step comes first"
    );
    harness
}

fn settings(dir: &Path, parent: &Path, comment: &str) -> Harness<'static, AppState> {
    let mut harness = local_changes(dir, parent);
    harness.state_mut().ui.dlg.create_patch_comment = comment.to_owned();
    settle(&mut harness);
    click(&mut harness, "OK");
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::PatchFileSettings),
        "the comment names the file, then the settings are asked about it"
    );
    harness
}

#[track_caller]
fn click(harness: &mut Harness<'_, AppState>, label: &str) {
    let node = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .chain(
            harness
                .get_all_by_role(egui::accesskit::Role::CheckBox)
                .collect::<Vec<_>>(),
        )
        .find(|n| n.accesskit_node().label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no control labelled `{label}` is painted"));
    node.click();
    harness.step();
}

#[track_caller]
fn click_counted(harness: &mut Harness<'_, AppState>, prefix: &str) {
    let node = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .as_deref()
                .is_some_and(|l| l.starts_with(prefix))
        })
        .unwrap_or_else(|| panic!("no control whose label starts with `{prefix}` is painted"));
    node.click();
    harness.step();
}

#[track_caller]
fn patch_paths(harness: &Harness<'_, AppState>) -> String {
    let painted = painted_text(harness);
    let at = painted
        .iter()
        .position(|t| t.contains("FILES THE PATCH NAMES"))
        .expect("the dialog paints the paths its settings leave behind");
    painted[at..].join("\n")
}

#[track_caller]
fn first_index(harness: &Harness<'_, AppState>, needle: &str) -> usize {
    painted_text(harness)
        .iter()
        .position(|t| t.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` was never painted"))
}

fn payload_text(harness: &Harness<'_, AppState>) -> String {
    let bytes = turbogit_app::create_patch::payload(harness.state())
        .expect("the settings describe a patch");
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn three_presses_paint_three_and_two_restores_paint_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let mut harness = settings(&dir, tmp.path(), "Repair the parser");

    assert_painted(&harness, "Remove Leading Directory [0]");
    assert!(
        patch_paths(&harness).contains(DEEP),
        "git's own answer first: {}",
        patch_paths(&harness)
    );

    let shown = ["main/java/App.java", "java/App.java", "App.java"];
    for (press, path) in shown.iter().enumerate() {
        click_counted(&mut harness, "Remove Leading Directory [");
        settle(&mut harness);
        let count = press + 1;
        assert!(
            painted(&harness, &format!("Remove Leading Directory [{count}]")),
            "the count is on the button that moved it, in the frame after the press"
        );
        assert!(
            patch_paths(&harness).contains(path),
            "press {count} rewrote every path to `{path}`: {}",
            patch_paths(&harness)
        );
        assert!(
            !painted(
                &harness,
                &format!("Remove Leading Directory [{}]", count + 1)
            ),
            "one press is one segment — the count never runs ahead of the press"
        );
    }

    click_counted(&mut harness, "Restore Leading Directory");
    settle(&mut harness);
    assert_painted(&harness, "Remove Leading Directory [2]");
    assert!(
        patch_paths(&harness).contains("java/App.java"),
        "one given back: {}",
        patch_paths(&harness)
    );
    click_counted(&mut harness, "Restore Leading Directory");
    settle(&mut harness);
    assert_painted(&harness, "Remove Leading Directory [1]");
    assert!(
        patch_paths(&harness).contains("main/java/App.java"),
        "and two: {}",
        patch_paths(&harness)
    );

    click(&mut harness, "Restore All Leading Directories");
    settle(&mut harness);
    assert_painted(&harness, "Remove Leading Directory [0]");
    for _ in 0..6 {
        click_counted(&mut harness, "Remove Leading Directory [");
        settle(&mut harness);
    }
    assert_painted(&harness, "Remove Leading Directory [3]");
    assert_not_painted(&harness, "Remove Leading Directory [4]");
    assert!(
        patch_paths(&harness).contains("App.java"),
        "the shallowest spelling is the file's own name, never nothing: {}",
        patch_paths(&harness)
    );
}

#[test]
fn the_two_all_buttons_are_the_same_counters_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let mut harness = settings(&dir, tmp.path(), "Flatten");

    click(&mut harness, "Remove All Leading Directories");
    settle(&mut harness);
    assert_painted(&harness, "Remove Leading Directory [3]");
    assert!(
        patch_paths(&harness).contains("App.java"),
        "all the way down: {}",
        patch_paths(&harness)
    );

    click(&mut harness, "Restore All Leading Directories");
    settle(&mut harness);
    assert_painted(&harness, "Remove Leading Directory [0]");
    assert!(
        patch_paths(&harness).contains(DEEP),
        "and all the way back: {}",
        patch_paths(&harness)
    );
}

#[test]
fn the_comment_becomes_the_name_of_the_file_that_gets_written() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let out = tmp.path().join("patches");
    std::fs::create_dir_all(&out).unwrap();

    let mut harness = local_changes(&dir, tmp.path());
    assert_painted(&harness, create_patch::COMMENT_PROMPT);
    harness
        .state_mut()
        .ui
        .dlg
        .create_patch_comment
        .push_str("Fix the login form");
    settle(&mut harness);
    assert_painted(&harness, "Fix_the_login_form.patch");

    click(&mut harness, "OK");
    settle(&mut harness);
    harness.state_mut().ui.dlg.create_patch_location = out.to_string_lossy().into_owned();
    settle(&mut harness);
    click(&mut harness, "OK");
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.dialog,
        None,
        "a written patch closes the flow"
    );
    let written = out.join("Fix_the_login_form.patch");
    let bytes = std::fs::read(&written)
        .unwrap_or_else(|e| panic!("`{}` was not written: {e}", written.display()));
    let text = String::from_utf8(bytes).expect("this patch is text");
    assert!(
        text.starts_with("diff --git") && text.contains("b/src/main/java/App.java"),
        "the file holds the patch the dialog described: {text}"
    );
    assert!(text.contains("+TWO"), "and the change itself: {text}");
    assert_painted(&harness, "Fix_the_login_form.patch");
    assert_painted(&harness, "Import to shelf");
    assert_eq!(
        harness
            .state()
            .ui
            .patch_created
            .as_ref()
            .map(|a| a.path.file_name().map(|n| n.to_string_lossy().into_owned())),
        Some(Some("Fix_the_login_form.patch".to_string())),
        "the default arm leaves the created patch waiting for an answer"
    );
}

#[test]
fn renaming_the_file_does_not_rename_the_comment() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let out = tmp.path().join("patches");
    std::fs::create_dir_all(&out).unwrap();

    let mut harness = settings(&dir, tmp.path(), "Fix the login form");
    harness.state_mut().ui.dlg.create_patch_name = "hand-picked.patch".to_owned();
    harness.state_mut().ui.dlg.create_patch_location = out.to_string_lossy().into_owned();
    settle(&mut harness);
    click(&mut harness, "OK");
    settle(&mut harness);

    assert!(
        out.join("hand-picked.patch").is_file(),
        "the edited name is the file that lands"
    );
    assert!(
        !out.join("Fix_the_login_form.patch").exists(),
        "and the derived one is not written alongside it"
    );
    assert_eq!(
        harness.state().ui.dlg.create_patch_comment,
        "Fix the login form",
        "the comment is untouched by the rename"
    );
}

#[test]
fn the_four_settings_fields_are_painted_in_the_references_order() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let harness = settings(&dir, tmp.path(), "Order");

    assert_painted(&harness, create_patch::SETTINGS_TITLE);
    let location = first_index(&harness, "PATCH FILE LOCATION");
    let clipboard = first_index(&harness, "To clipboard");
    let base = first_index(&harness, "BASE PATH");
    let reverse = first_index(&harness, "Reverse patch");
    let encoding = first_index(&harness, "Encoding");
    assert!(
        location < clipboard && clipboard < base && base < reverse && reverse < encoding,
        "location, `To clipboard`, base path, reverse, encoding — painted as \
         {location}, {clipboard}, {base}, {reverse}, {encoding}"
    );
    assert_painted(&harness, "Save to…");
    assert_painted(&harness, "Directory");
    assert_painted(&harness, "Name");
    assert_painted(&harness, "the project directory");
    assert_painted(&harness, "UTF-8");
    assert_painted(&harness, "CHANGE DIRECTORY PATHS");
}

#[test]
fn the_base_path_moves_the_paths_the_dialog_paints() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let mut harness = settings(&dir, tmp.path(), "Base");

    assert!(patch_paths(&harness).contains(DEEP));
    harness.state_mut().ui.dlg.create_patch_base_path = "..".to_owned();
    settle(&mut harness);
    assert!(
        patch_paths(&harness).contains("app/src/main/java/App.java"),
        "the base is one up, so every path says which project it came from: {}",
        patch_paths(&harness)
    );

    harness.state_mut().ui.dlg.create_patch_base_path = "src/main".to_owned();
    settle(&mut harness);
    assert!(
        patch_paths(&harness).contains("java/App.java"),
        "and a base inside the project strips its own prefix: {}",
        patch_paths(&harness)
    );
}

#[test]
fn the_reverse_field_changes_the_bytes_the_settings_hand_over() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let mut harness = settings(&dir, tmp.path(), "Revert me");

    click(&mut harness, "Reverse patch");
    settle(&mut harness);
    let payload = payload_text(&harness);
    assert!(
        payload.contains("-TWO"),
        "the patch removes what the worktree added: {payload}"
    );
    assert!(
        payload.contains("+two"),
        "and restores what HEAD holds: {payload}"
    );
}

#[test]
fn the_encoding_field_is_read_back_off_the_written_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let out = tmp.path().join("patches");
    std::fs::create_dir_all(&out).unwrap();

    let mut harness = settings(&dir, tmp.path(), "Encoded");
    harness.state_mut().ui.dlg.create_patch_location = out.to_string_lossy().into_owned();
    harness.state_mut().ui.dlg.create_patch_encoding = PatchEncoding::Utf8WithBom;
    settle(&mut harness);
    click(&mut harness, "OK");
    settle(&mut harness);

    let bytes = std::fs::read(out.join("Encoded.patch")).expect("the patch is written");
    assert_eq!(
        &bytes[..3],
        &[0xEF, 0xBB, 0xBF],
        "the BOM is the field's whole effect, and it is on the file"
    );
    assert!(
        String::from_utf8_lossy(&bytes[3..]).contains("+TWO"),
        "behind it, git's own bytes, unchanged"
    );
}

#[test]
fn the_clipboard_route_opens_no_dialog_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let state = state_over(std::slice::from_ref(&dir), tmp.path());
    let mut harness = shell_harness_over(state, Vec2::new(1280.0, 900.0));
    // The verb runs against the live shell, so the copy lands in a frame this test can
    // see: an output command belongs to the frame that emitted it, and a harness built
    // after the verb has already run has stepped past its own evidence.
    run_action(harness.state_mut(), Action::CopyPatchToClipboard);

    let copied = copied_across_frames(&mut harness, 3)
        .expect("the frame that owns the clipboard performed the copy");
    assert!(
        copied.contains("diff --git") && copied.contains("+TWO"),
        "and what it holds is the patch: {copied}"
    );
    assert_eq!(
        harness.state().ui.dialog,
        None,
        "the verb asks nothing: no comment step, no settings"
    );
    assert_not_painted(&harness, create_patch::SETTINGS_TITLE);
    assert_not_painted(&harness, create_patch::COMMENT_PROMPT);
    settle(&mut harness);
    assert_painted(&harness, "Patch copied to the clipboard");
}

fn painted(harness: &Harness<'_, AppState>, needle: &str) -> bool {
    painted_text(harness).iter().any(|t| t.contains(needle))
}

fn copied_text(harness: &Harness<'_, AppState>) -> Option<String> {
    harness
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
}

/// The text the shell handed the clipboard across the next `frames` frames.
///
/// Collected frame by frame rather than read off the settled frame, because an output
/// command belongs to the one frame that emitted it: a suite that settled first would
/// have stepped past the copy and be asserting against its own timing.
fn copied_across_frames(harness: &mut Harness<'_, AppState>, frames: usize) -> Option<String> {
    for _ in 0..frames {
        harness.step();
        if let Some(text) = copied_text(harness) {
            return Some(text);
        }
    }
    None
}

#[test]
fn a_patch_that_is_not_text_refuses_the_clipboard_rather_than_losing_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = clean_repo(tmp.path());
    std::fs::write(dir.join(DEEP), [b'x', 0xff, 0xfe, b'\n']).unwrap();
    let state = state_over(std::slice::from_ref(&dir), tmp.path());
    let mut harness = shell_harness_over(state, Vec2::new(1280.0, 900.0));
    run_action(harness.state_mut(), Action::CopyPatchToClipboard);

    assert!(
        copied_across_frames(&mut harness, 6).is_none(),
        "the refused copy never reaches the clipboard"
    );
    assert_painted(&harness, "not valid UTF-8");
    assert_eq!(
        harness.state().ui.dialog,
        None,
        "a refusal here is a toast, not a dialog that opened and then failed"
    );
}

#[test]
fn a_log_commit_skips_the_comment_and_names_the_file_from_its_subject() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = clean_repo(tmp.path());
    let root = root_id(&dir);
    let id = head(&dir);
    let mut state = state_over(std::slice::from_ref(&dir), tmp.path());
    state.caches.store_log(
        root.clone(),
        vec![cached(&root, &id, "Align the parser output")],
    );
    turbogit_app::create_patch::open_on(
        &mut state,
        root.clone(),
        PatchOrigin::Commit { id: id.clone() },
    );
    let harness = settled(state);

    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::PatchFileSettings),
        "the commit's own subject is the name the comment step would have asked for"
    );
    assert_eq!(
        harness.state().ui.dlg.create_patch_name,
        "Align_the_parser_output.patch"
    );
    assert_painted(&harness, create_patch::SETTINGS_TITLE);
    assert_not_painted(&harness, create_patch::COMMENT_PROMPT);
    let paths = patch_paths(&harness);
    assert!(paths.contains(DEEP), "the commit's file is named: {paths}");
    assert!(
        !paths.contains("README.md"),
        "an ancestor's path does not ride along: {paths}"
    );
}

#[test]
fn a_file_revision_routes_straight_to_the_settings_naming_one_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = clean_repo(tmp.path());
    let root = root_id(&dir);
    let id = head(&dir);
    let mut state = state_over(std::slice::from_ref(&dir), tmp.path());
    state.caches.store_log(
        root.clone(),
        vec![cached(&root, &id, "Align the parser output")],
    );
    turbogit_app::create_patch::open_on(
        &mut state,
        root.clone(),
        PatchOrigin::FileRevision {
            commit: id.clone(),
            path: PathBuf::from(DEEP),
        },
    );
    let harness = settled(state);

    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::PatchFileSettings),
        "a revision names itself, so there is no comment to ask for"
    );
    assert_painted(&harness, DEEP);
    assert_eq!(
        payload_text(&harness).matches("diff --git").count(),
        1,
        "one section, for the one file the row named"
    );
}

#[test]
fn the_create_verbs_are_palette_routes_and_the_popup_stays_frozen() {
    let palette: Vec<&str> = Action::palette_actions()
        .iter()
        .map(|action| action.label())
        .collect();
    let popup: Vec<&str> = Action::all().iter().map(|action| action.label()).collect();
    for route in [
        create_patch::LOCAL_CHANGES_VERB,
        create_patch::CLIPBOARD_VERB,
    ] {
        assert!(
            palette.contains(&route),
            "`{route}` is unreachable: {palette:?}"
        );
        assert!(
            !popup.contains(&route),
            "the VCS popup's documented action set stays frozen"
        );
    }
}

#[test]
fn a_checked_file_narrows_the_patch_to_that_file_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dirty_repo(tmp.path());
    let mut state = state_over(std::slice::from_ref(&dir), tmp.path());
    let root = &state.multi.roots[0];
    let key = root
        .status
        .changes
        .iter()
        .find(|c| c.path.ends_with("Util.java"))
        .map(|c| root.canonical_key(c))
        .expect("Util.java is a local change");
    state.ui.selected.insert(key);
    turbogit_app::create_patch::open_from_selection(&mut state);
    let mut harness = settled(state);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::CreatePatchComment),
        "a checked file is still local changes, so the comment still names it"
    );
    click(&mut harness, "OK");
    settle(&mut harness);

    let paths = patch_paths(&harness);
    assert!(
        paths.contains("Util.java"),
        "the checked file is in it: {paths}"
    );
    assert!(
        !paths.contains("App.java"),
        "and the unchecked change is not: {paths}"
    );
    let mut nothing_checked = state_over(std::slice::from_ref(&dir), tmp.path());
    turbogit_app::create_patch::open_from_selection(&mut nothing_checked);
    assert_eq!(
        nothing_checked.ui.dlg.create_patch_origin,
        Some(PatchOrigin::LocalChanges { paths: vec![] }),
        "nothing checked is every local change, not none of them"
    );
    assert_eq!(
        nothing_checked.ui.dialog,
        Some(Dialog::CreatePatchComment),
        "and it opens the same first step"
    );
}

#[test]
fn switching_the_create_root_drops_a_count_measured_against_another_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let first = dirty_repo(tmp.path());
    let second = tmp.path().join("other");
    std::fs::create_dir_all(&second).unwrap();
    git(&second, &["init", "-q", "-b", "main"]);
    git(&second, &["config", "user.email", "t@t"]);
    git(&second, &["config", "user.name", "t"]);
    git_commit(&second, "shallow.txt", "one\n", "seed");
    std::fs::write(second.join("shallow.txt"), "one\ntwo\n").unwrap();

    let mut harness = settings(&first, tmp.path(), "Two trees");
    click_counted(&mut harness, "Remove Leading Directory [");
    settle(&mut harness);
    assert_painted(&harness, "Remove Leading Directory [1]");

    let other = root_id(&second);
    turbogit_app::create_patch::set_root(harness.state_mut(), &other);
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.dlg.create_patch_strip_leading,
        0,
        "the count came back to zero with the patch it counted"
    );
    assert_eq!(
        harness.state().ui.dlg.create_patch_removable,
        0,
        "a shallow path can lose nothing, and the bound moved with the root"
    );
    assert_painted(&harness, "Remove Leading Directory [0]");
    assert!(
        patch_paths(&harness).contains("shallow.txt"),
        "and the paths are the other tree's: {}",
        patch_paths(&harness)
    );
}
