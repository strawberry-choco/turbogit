//! Apply patch, ported from the egui `apply_patch` suite: the two sources,
//! the preflight verdict, the provenance, the aborted outcome, the
//! malformed patch's file and line, and the import to the shelf.

use std::path::{Path, PathBuf};

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::{commit, git};
use turbogit_app::state::{AppState, Dialog, PatchSource};

mod support;

fn repo(project: &Project) -> PathBuf {
    let repo = project.path().join("app");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "ada@example.com"]);
    git(&repo, &["config", "user.name", "Ada Lovelace"]);
    commit(&repo, "first.txt", "one\ntwo\nthree\n", "first.txt");
    commit(&repo, "second.txt", "alpha\nbeta\n", "second.txt");
    repo
}

fn conflicting_patch(project: &Project) -> PathBuf {
    let path = project.path().join("change.patch");
    std::fs::write(
        &path,
        "diff --git a/first.txt b/first.txt\n--- a/first.txt\n+++ b/first.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\ndiff --git a/second.txt b/second.txt\n--- a/second.txt\n+++ b/second.txt\n@@ -1,4 +1,4 @@\n alpha\n-beta\n+BETA\n gamma\n delta\n",
    )
    .unwrap();
    path
}

fn format_patch(project: &Project, repo: &Path) -> PathBuf {
    git(repo, &["format-patch", "-1", "-o", "out"]);
    let written = std::fs::read_dir(repo.join("out"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "patch"))
        .expect("format-patch wrote a patch file");
    let dest = project.path().join("provenance.patch");
    std::fs::copy(&written, &dest).unwrap();
    dest
}

fn apply_over(
    project: &Project,
    repo: &Path,
    source: PatchSource,
    path: Option<PathBuf>,
) -> Mounted {
    let roots = vec![repo.to_path_buf()];
    let mut app = for_roots_seeded(project.path(), &roots, move |s: &mut AppState| {
        s.ui.dlg.apply_patch_root = Some(s.multi.roots[0].id.clone());
        s.ui.dlg.apply_patch_source = source;
        if let Some(path) = path {
            s.ui.dlg.apply_patch_path = path.to_string_lossy().into_owned();
        }
        turbogit_app::apply_patch::sync(s);
        s.ui.dialog = Some(Dialog::ApplyPatch);
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:apply-patch")
        .expect("the apply dialog paints");
    app
}

#[test]
fn the_clipboard_route_skips_the_file_path_the_file_route_asks_for() {
    let project = Project::new();
    let repo = repo(&project);
    let file_route = apply_over(&project, &repo, PatchSource::File, None);
    assert!(support::shows_part(&file_route, "Choose file"));
    assert!(!support::shows_part(
        &file_route,
        "Paste the patch below — no file is read."
    ));
    let clipboard = apply_over(&project, &repo, PatchSource::Clipboard, None);
    assert!(!support::shows_part(&clipboard, "Choose file"));
    assert!(support::shows_part(
        &clipboard,
        "Paste the patch below — no file is read."
    ));
}

#[test]
fn show_diff_returns_gits_verdict_without_writing() {
    let project = Project::new();
    let repo = repo(&project);
    let patch = conflicting_patch(&project);
    let before = std::fs::read(repo.join("first.txt")).unwrap();
    let mut app = apply_over(&project, &repo, PatchSource::File, Some(patch));
    app.wait_for("text:/FILES THE PATCH NAMES · 2/")
        .expect("the patch's files paint");
    app.press("id:patch:show-diff").expect("show diff");
    app.wait_for("text:The patch does not apply cleanly. No file has been changed.")
        .expect("the refusal paints");
    assert_eq!(
        std::fs::read(repo.join("first.txt")).unwrap(),
        before,
        "the preflight wrote nothing"
    );
    assert!(
        git(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "the preflight dirtied nothing"
    );
}

#[test]
fn a_git_format_patch_fills_the_author_and_message() {
    let project = Project::new();
    let repo = repo(&project);
    commit(&repo, "note.txt", "raised\n", "Raise the note");
    let patch = format_patch(&project, &repo);
    let app = apply_over(&project, &repo, PatchSource::File, Some(patch));
    assert!(support::shows_part(&app, "PATCH PROVENANCE"));
    assert!(
        support::shows_part(&app, "Ada Lovelace <ada@example.com>"),
        "the author the patch names paints in its editable field"
    );
    assert!(support::shows_part(&app, "Raise the note"));
}

#[test]
fn a_patch_with_no_header_says_it_has_none() {
    let project = Project::new();
    let repo = repo(&project);
    let patch = conflicting_patch(&project);
    let app = apply_over(&project, &repo, PatchSource::File, Some(patch));
    assert!(support::shows_part(
        &app,
        "This patch carries no commit header, so there is nothing to fill."
    ));
}

#[test]
fn a_failed_apply_paints_the_aborted_state_over_a_restored_tree() {
    let project = Project::new();
    let repo = repo(&project);
    let patch = conflicting_patch(&project);
    let before_first = std::fs::read(repo.join("first.txt")).unwrap();
    let before_second = std::fs::read(repo.join("second.txt")).unwrap();
    let mut app = apply_over(&project, &repo, PatchSource::File, Some(patch));
    app.press("id:patch:apply").expect("apply");
    app.wait_for("id:patch:outcome")
        .expect("the outcome panel paints");
    assert!(support::shows_part(&app, "Apply patch aborted"));
    assert!(support::shows_part(&app, "ROLLED BACK · 2"));
    assert_eq!(std::fs::read(repo.join("first.txt")).unwrap(), before_first);
    assert_eq!(
        std::fs::read(repo.join("second.txt")).unwrap(),
        before_second
    );
}

#[test]
fn a_malformed_patch_names_its_file_and_line() {
    let project = Project::new();
    let repo = repo(&project);
    let patch = conflicting_patch(&project);
    let mut app = apply_over(&project, &repo, PatchSource::File, Some(patch));
    app.press("id:patch:show-diff").expect("show diff");
    app.wait_for("text:The patch does not apply cleanly. No file has been changed.")
        .unwrap();
    let texts = app.screen().texts();
    let named = texts.iter().find(|text| {
        (text.contains("first.txt") || text.contains("second.txt"))
            && text.chars().any(|c| c.is_ascii_digit())
    });
    assert!(
        named.is_some(),
        "the report names a file and a line: {texts:?}"
    );
}

#[test]
fn importing_to_the_shelf_parks_the_patch_without_applying_it() {
    let project = Project::new();
    let repo = repo(&project);
    let patch = conflicting_patch(&project);
    let mut app = apply_over(&project, &repo, PatchSource::File, Some(patch));
    app.press("id:patch:import-to-shelf")
        .expect("import to shelf");
    app.wait_for("text:/on the shelf/")
        .expect("the import reports its outcome");
    assert!(
        git(&repo, &["status", "--porcelain"]).trim().is_empty(),
        "nothing was applied"
    );
}
