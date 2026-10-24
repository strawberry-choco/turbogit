//! A `state.ron` naming a **retired settings key** still loads, and a retired
//! key changes nothing about the engine the app runs.
//!
//! Removing a persisted field is a serialization migration, not just a UI
//! deletion: every project on disk has the key, and if the loader rejected
//! unknown keys the user would come back to a reset settings block — which the
//! next settings apply would then save back over their own file. The
//! `restore_workspace` sentinel in every fixture proves the file *parsed*
//! rather than a parse failure falling back to defaults, which is the failure
//! these tests exist for.
//!
//! The backend key gets the strongest form of the proof (ADR-0030): the engine
//! is the composed adapter whatever an older file's `backend:` line said, so a
//! saved `Cli` spelling must not bring the CLI adapter's git-DWIM behavior
//! back.

use std::fs;
use std::path::{Path, PathBuf};

use test_support::git_seed::{GitEnv, git_with_env};
use turbogit_app::persistence;
use turbogit_domain::model::VcsSettings;
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

/// Persist settings, then re-insert `key_line` exactly as an older build wrote
/// it — the bytes a retired key survives as on disk.
fn saved_with_key(project: &Path, key_line: &str) {
    let settings = VcsSettings {
        restore_workspace: true,
        ..VcsSettings::default()
    };
    persistence::save_settings(project, &settings).expect("settings saved");
    let path = persistence::state_path(project);
    let text = fs::read_to_string(&path).expect("state.ron readable");
    let patched = text.replace(
        "warn_crlf: true",
        &format!("warn_crlf: true,\n    {key_line}"),
    );
    assert!(
        patched.contains(key_line),
        "the serialized settings name `warn_crlf` on their own line: {text}"
    );
    fs::write(&path, patched).expect("state.ron rewritten");
}

/// Every spelling the retired backend key ever serialized as, including the
/// two aliases `ADR-0022` collapsed. None of them may reject the file, and
/// none of them may change what loads.
#[test]
fn a_saved_backend_spelling_still_loads_whatever_it_says() {
    for spelling in ["Auto", "Libgit2", "Cli", "InProcessReads"] {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path();
        saved_with_key(project, &format!("backend: {spelling}"));

        let loaded = persistence::load_settings(project).expect("an old state.ron must still load");
        assert!(
            loaded.restore_workspace,
            "`{spelling}`: the file parsed — this is the settings on disk, not a \
             default"
        );
    }
}

/// The retired key is not merely ignored on load: the engine it leaves behind
/// is the composed one. The probe is git's DWIM on `branch_create` from a
/// remote start point — the one read where the CLI and composed adapters
/// visibly differ (`backend_options.rs` pins the pair), so a saved `Cli`
/// spelling that resurrected the CLI adapter would record an upstream here.
#[test]
fn a_saved_cli_spelling_does_not_bring_the_cli_adapter_back() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    saved_with_key(project, "backend: Cli");

    let loaded = persistence::load_settings(project).expect("the old state.ron loads");
    let (_remote_tmp, work) = repo_with_remote_branch();
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
        "the composed adapter answers whatever the retired key said: the CLI \
         adapter would have recorded origin/feat through git's DWIM"
    );
}

/// **A `state.ron` written before `commit_all_when_nothing_staged` existed
/// still loads.**
#[test]
fn a_saved_commit_all_setting_still_loads_and_the_rest_of_the_file_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    saved_with_key(project, "commit_all_when_nothing_staged: true");

    let loaded = persistence::load_settings(project).expect(
        "a settings file naming a retired field must still load — the loader \
         ignores fields it no longer has",
    );
    assert!(
        loaded.restore_workspace,
        "the file parsed: this is the settings on disk, not a default"
    );
}
