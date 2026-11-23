//! Create patch, ported from the egui `create_patch` suite: the comment
//! step, the settings fields and their order, the directory counters, the
//! reverse and base path, the encoding, and the file that lands.

use std::path::{Path, PathBuf};

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::{commit, git};
use turbogit_app::create_patch;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{PatchEncoding, PatchOrigin};

mod support;

const DEEP: &str = "src/main/java/App.java";

fn dirty_repo(project: &Project) -> PathBuf {
    let repo = project.path().join("app");
    std::fs::create_dir_all(repo.join("src/main/java")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "ada@example.com"]);
    git(&repo, &["config", "user.name", "Ada Lovelace"]);
    commit(&repo, "README.md", "readme\n", "seed");
    commit(&repo, DEEP, "one\ntwo\nthree\n", "Align the parser output");
    std::fs::write(repo.join(DEEP), "one\nTWO\nthree\n").unwrap();
    repo
}

fn settings_over(
    project: &Project,
    repo: &Path,
    comment: &str,
    configure: impl FnOnce(&mut AppState) + Send + 'static,
) -> Mounted {
    let roots = vec![repo.to_path_buf()];
    let comment = comment.to_string();
    let mut app = for_roots_seeded(project.path(), &roots, move |s| {
        let root = s.multi.roots[0].id.clone();
        s.ui.dlg.create_patch_root = Some(root);
        s.ui.dlg.create_patch_origin = Some(PatchOrigin::LocalChanges { paths: Vec::new() });
        s.ui.dlg.create_patch_comment = comment;
        create_patch::from_comment(s);
        configure(s);
        create_patch::sync(s);
        s.ui.dialog = Some(Dialog::PatchFileSettings);
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:patch-file-settings")
        .expect("the settings dialog paints");
    app
}

fn painted_paths(app: &Mounted) -> String {
    app.screen().texts().join("\n")
}

#[test]
fn three_presses_paint_three_and_two_restores_paint_one() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let mut app = settings_over(&project, &repo, "Repair the parser", |_| {});
    assert!(support::shows_part(&app, "Remove Leading Directory [0]"));
    assert!(painted_paths(&app).contains(DEEP));
    for press in 1..=3 {
        app.press("id:patch:remove-leading")
            .expect("remove leading");
        app.wait_for(&format!("text:Remove Leading Directory [{press}]"))
            .expect("the counter follows the press");
    }
    assert!(painted_paths(&app).contains("App.java"));
    assert!(!painted_paths(&app).contains(DEEP));
    app.press("id:patch:restore-leading")
        .expect("restore leading");
    app.wait_for("text:Remove Leading Directory [2]")
        .expect("one directory comes back");
    app.press("id:patch:restore-all-leading")
        .expect("restore all");
    app.wait_for("text:Remove Leading Directory [0]")
        .expect("the counter returns to zero");
    assert!(painted_paths(&app).contains(DEEP));
}

#[test]
fn the_two_all_buttons_are_the_counters_ends() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let mut app = settings_over(&project, &repo, "Flatten", |_| {});
    app.press("id:patch:remove-all-leading")
        .expect("remove all");
    app.wait_for("text:Remove Leading Directory [3]")
        .expect("all the way down");
    assert!(painted_paths(&app).contains("App.java"));
    app.press("id:patch:restore-all-leading")
        .expect("restore all");
    app.wait_for("text:Remove Leading Directory [0]")
        .expect("all the way back");
    assert!(painted_paths(&app).contains(DEEP));
}

#[test]
fn the_comment_becomes_the_name_of_the_file_that_gets_written() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let out = project.path().join("patches");
    std::fs::create_dir_all(&out).unwrap();
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&repo), move |s| {
        let root = s.multi.roots[0].id.clone();
        s.ui.dlg.create_patch_root = Some(root);
        s.ui.dlg.create_patch_origin = Some(PatchOrigin::LocalChanges { paths: Vec::new() });
        s.ui.dlg.create_patch_comment = "Fix the login form".to_string();
        s.ui.dialog = Some(Dialog::CreatePatchComment);
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:create-patch-comment")
        .expect("the comment step paints");
    assert!(support::shows_part(
        &app,
        "Enter a commit comment (it will be used as the patch file name)"
    ));
    assert!(support::shows_part(&app, "Fix_the_login_form.patch"));
    app.press("id:dialog:create-patch-comment:action")
        .expect("the comment is confirmed");
    app.wait_for("id:dialog:patch-file-settings")
        .expect("the settings follow the comment");
    app.clear("id:patch:location").expect("clear the location");
    app.paste(
        "id:patch:location",
        &nsh_core::PasteEvent::new(out.to_string_lossy().to_string()),
    )
    .expect("type the output directory");
    app.press("id:patch:settings-ok").expect("write the patch");
    let written = out.join("Fix_the_login_form.patch");
    assert!(
        test_support::wait_until(10_000, || written.is_file()),
        "the comment-named file lands"
    );
    let text = std::fs::read_to_string(&written).unwrap();
    assert!(text.starts_with("diff --git"));
    assert!(text.contains("b/src/main/java/App.java"));
    assert!(text.contains("+TWO"));
}

#[test]
fn the_four_settings_fields_are_painted_in_the_references_order() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let app = settings_over(&project, &repo, "Order", |_| {});
    let texts = app.screen().texts();
    let at = |needle: &str| texts.iter().position(|text| *text == needle);
    let location = at("PATCH FILE LOCATION").expect("location");
    let clipboard = at("To clipboard").expect("clipboard");
    let base = at("BASE PATH").expect("base");
    let reverse = at("Reverse patch").expect("reverse");
    let encoding = at("Encoding").expect("encoding");
    assert!(
        location < clipboard && clipboard < base && base < reverse && reverse < encoding,
        "location, clipboard, base, reverse, encoding paint in order"
    );
    assert!(support::shows_part(&app, "Save to…"));
    assert!(support::shows_part(&app, "UTF-8"));
    assert!(support::shows_part(&app, "CHANGE DIRECTORY PATHS"));
}

#[test]
fn the_base_path_moves_the_paths_the_dialog_paints() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let app = settings_over(&project, &repo, "Base", |s| {
        s.ui.dlg.create_patch_base_path = "..".to_string();
    });
    assert!(
        painted_paths(&app).contains("app/src/main/java/App.java"),
        "the base is one up: {}",
        painted_paths(&app)
    );
}

#[test]
fn the_reverse_field_changes_the_bytes_the_settings_hand_over() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let out = project.path().join("patches");
    std::fs::create_dir_all(&out).unwrap();
    let out_for_seed = out.clone();
    let mut app = settings_over(&project, &repo, "Revert me", move |s| {
        s.ui.dlg.create_patch_location = out_for_seed.to_string_lossy().into_owned();
    });
    app.press("id:patch:reverse").expect("reverse");
    app.press("id:patch:settings-ok").expect("write the patch");
    let written = out.join("Revert_me.patch");
    assert!(test_support::wait_until(10_000, || written.is_file()));
    let payload = std::fs::read_to_string(&written).unwrap();
    assert!(
        payload.contains("-TWO"),
        "the patch removes the worktree line"
    );
    assert!(payload.contains("+two"), "and restores what HEAD holds");
}

#[test]
fn the_encoding_field_is_read_back_off_the_written_file() {
    let project = Project::new();
    let repo = dirty_repo(&project);
    let out = project.path().join("patches");
    std::fs::create_dir_all(&out).unwrap();
    let out_for_seed = out.clone();
    let mut app = settings_over(&project, &repo, "Encoded", move |s| {
        s.ui.dlg.create_patch_location = out_for_seed.to_string_lossy().into_owned();
        s.ui.dlg.create_patch_encoding = PatchEncoding::Utf8WithBom;
    });
    app.press("id:patch:settings-ok").expect("write the patch");
    let written = out.join("Encoded.patch");
    assert!(test_support::wait_until(10_000, || written.is_file()));
    let bytes = std::fs::read(&written).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "the BOM is on the file");
    assert!(String::from_utf8_lossy(&bytes[3..]).contains("+TWO"));
}
