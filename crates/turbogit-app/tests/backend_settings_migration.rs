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

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::persistence;
use turbogit_domain::model::{GitBackend, VcsSettings};
use turbogit_engine::build_executor;

// No repository `user.*` here, so every commit this fixture makes is pinned per
// call through the fixture crate's runner: a headless machine has no global
// identity to borrow.

/// One repo with a same-content `origin`, plus `feat` pushed there and deleted
/// locally — so a branch created from `origin/feat` shows which adapter answered.
fn repo_with_remote_branch() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let bare = project.join("origin.git");
    git_with_env(
        &project,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    let work = project.join("work");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(work.join("README.md"), "x\n").unwrap();
    git_with_env(&work, &["add", "."], &GitEnv::fixture());
    git_with_env(&work, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    git_with_env(
        &work,
        &["remote", "add", "origin", bare.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    git_with_env(
        &work,
        &["push", "-q", "-u", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(&work, &["branch", "feat", "HEAD"], &GitEnv::fixture());
    git_with_env(&work, &["push", "-q", "origin", "feat"], &GitEnv::fixture());
    git_with_env(&work, &["branch", "-D", "feat"], &GitEnv::fixture());
    (tmp, work)
}

/// Persist settings, then rewrite the file so the backend reads `spelling` —
/// exactly the bytes an older build left behind. The `restore_workspace`
/// sentinel (a non-default value) proves the file really parsed rather than a
/// parse failure falling back to defaults.
fn saved_with_spelling(project: &Path, spelling: &str) {
    let settings = VcsSettings {
        restore_workspace: true,
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
    assert!(
        loaded.restore_workspace,
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
    assert!(
        loaded.restore_workspace,
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

/// **A `state.ron` written before `commit_all_when_nothing_staged` existed
/// still loads.** Removing a persisted field is a serialization migration, not
/// just a UI deletion: every project on disk has the key, and if the loader
/// rejected unknown keys the user would come back to a reset settings block —
/// which the next settings apply would then save back over their own file.
///
/// The sentinel is the point: it proves the file *parsed* rather than a parse
/// failure falling back to defaults, which is the failure this test exists for.
#[test]
fn a_saved_commit_all_setting_still_loads_and_the_rest_of_the_file_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    let settings = VcsSettings {
        restore_workspace: true,
        ..VcsSettings::default()
    };
    persistence::save_settings(project, &settings).expect("settings saved");

    // Re-insert the retired key exactly as an older build wrote it.
    let path = persistence::state_path(project);
    let text = fs::read_to_string(&path).expect("state.ron readable");
    let patched = text.replace(
        "warn_crlf: true",
        "warn_crlf: true,\n    commit_all_when_nothing_staged: true",
    );
    assert!(
        patched != text,
        "the serialized settings name `warn_crlf` on their own line: {text}"
    );
    fs::write(&path, patched).expect("state.ron rewritten");

    let loaded = persistence::load_settings(project).expect(
        "a settings file naming a retired field must still load — the loader \
         ignores fields it no longer has",
    );
    assert!(
        loaded.restore_workspace,
        "the file parsed: this is the settings on disk, not a default"
    );
}
