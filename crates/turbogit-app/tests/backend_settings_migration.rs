//! Issue 02 (deepen-git-engine) — a saved configuration naming the retired
//! backend spelling still loads, and lands on the adapter it already got.
//!
//! Two of the picker's three options constructed the same object, so collapsing
//! them is a *serialization* migration, not just a UI change: every existing
//! project has one of the retired spellings in its `state.ron`. A silent reset
//! there would change which engine a user was running, which is why the proof
//! here reads the old spelling off disk and asserts the executor the app builds
//! is the composed one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_app::persistence;
use turbogit_domain::model::{GitBackend, VcsSettings};
use turbogit_engine::build_executor;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One repo with a same-content `origin`, plus `feat` pushed there and deleted
/// locally — so a branch created from `origin/feat` shows which adapter answered.
fn repo_with_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    let work = project.join("work");
    git(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&work, &["branch", "feat", "HEAD"]);
    git(&work, &["push", "-q", "origin", "feat"]);
    git(&work, &["branch", "-D", "feat"]);
    (tmp, work)
}

/// Persist settings, then rewrite the file so the backend reads `spelling` —
/// exactly the bytes an older build left behind. The `commit_template` sentinel
/// proves the file really parsed rather than a parse failure falling back to
/// defaults.
fn saved_with_spelling(project: &Path, spelling: &str) {
    let settings = VcsSettings {
        commit_template: "/sentinel/commit-template".to_string(),
        ..VcsSettings::default()
    };
    persistence::save_settings(project, &settings).expect("settings saved");
    let path = persistence::state_path(project);
    let text = fs::read_to_string(&path).expect("state.ron readable");
    let current = ron::ser::to_string(&settings.backend).expect("backend spellable");
    let patched = text.replace(
        &format!("backend: {current}"),
        &format!("backend: {spelling}"),
    );
    assert!(
        patched.contains(&format!("backend: {spelling}")),
        "the serialized settings name {current} on their own line: {text}"
    );
    fs::write(&path, patched).expect("state.ron rewritten");
}

#[test]
fn a_saved_auto_spelling_loads_and_still_builds_the_composed_adapter() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    saved_with_spelling(project, "Auto");

    let loaded = persistence::load_settings(project).expect("an old state.ron must still load");
    assert_eq!(
        loaded.commit_template, "/sentinel/commit-template",
        "the file parsed — this is the settings on disk, not a default"
    );
    assert_eq!(
        loaded.backend,
        GitBackend::InProcessReads,
        "`Auto` named the composed adapter, so it must land on the survivor that does"
    );

    let (_repo, work) = repo_with_remote_branch();
    let exec = build_executor(&loaded);
    exec.branch_create(&work, "feat", true, Some("origin/feat"))
        .expect("creating feat from origin/feat");
    let tracked = exec
        .branches(&work)
        .unwrap()
        .into_iter()
        .find(|b| b.name == "feat")
        .and_then(|b| b.tracking);
    assert_eq!(
        tracked, None,
        "the migrated setting must build the same executor the user had: the CLI \
         adapter would have recorded origin/feat through git's DWIM"
    );
}

#[test]
fn a_saved_libgit2_spelling_loads_onto_the_same_survivor() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    saved_with_spelling(project, "Libgit2");

    let loaded = persistence::load_settings(project).expect("an old state.ron must still load");
    assert_eq!(loaded.backend, GitBackend::InProcessReads);
    assert_eq!(
        loaded.commit_template, "/sentinel/commit-template",
        "the rest of the file parsed too — this is the loaded settings, not a default"
    );
}

#[test]
fn a_saved_cli_spelling_is_not_swept_into_the_survivor() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    saved_with_spelling(project, "Cli");

    let loaded = persistence::load_settings(project).unwrap();
    assert_eq!(loaded.backend, GitBackend::Cli);
}
