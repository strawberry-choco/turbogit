//! Opening a repository hides this app's own `.turbogit/` from `git status`.
//!
//! The store is untracked by design (see `crate::persistence`), so the fix is
//! the clone's own `info/exclude`, reached through the engine — and the whole
//! product claim is about what the changes list shows. These suites drive the
//! launch and open paths production uses, write the store the way a real session
//! does, and then ask git itself.
//!
//! The order matters and is the design: the rule is listed at registration,
//! *before* `.turbogit/` exists, so the directory is never briefly untracked
//! between the first frame and the first durable write.
//!
//! The "where it makes sense" half is the second group. Root discovery returns
//! `project_dir` or a directory beneath it, so a workspace of sibling
//! repositories has its store *above* every root it registers — those
//! repositories cannot see it, and gain no rule written into their `.git`.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_app::state::AppState;

/// A real repository with one commit, so `HEAD` and `status` are readable.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "base.txt"]);
    git(&path, &["commit", "-q", "-m", "init"]);
    path
}

/// A config dir for the global recents file, kept out of the project directory
/// so this suite's assertions are about the repository, not the app's home.
fn config_dir(tmp: &TempDir) -> PathBuf {
    let cfg = tmp.path().join("config");
    std::fs::create_dir_all(&cfg).unwrap();
    cfg
}

/// The state of a session that has written its project store: the durable
/// write is a production trigger (a tab click), and it is what creates
/// `.turbogit/`.
fn open_with_store(project: &Path, cfg: &Path) -> AppState {
    let state = AppState::launch_in(Some(project.to_path_buf()), Some(cfg.to_path_buf()));
    state.save_durable_ui();
    state
}

fn exclude_file(repo: &Path) -> PathBuf {
    repo.join(".git").join("info").join("exclude")
}

fn listed(repo: &Path) -> bool {
    std::fs::read_to_string(exclude_file(repo))
        .unwrap_or_default()
        .lines()
        .any(|line| line.trim() == ".turbogit/")
}

#[test]
fn launching_a_repository_hides_the_store_it_is_about_to_write() {
    let tmp = TempDir::new().unwrap();
    let repo = temp_repo(tmp.path(), "repo");
    assert!(
        !repo.join(".turbogit").exists(),
        "the store must not exist before the open, or this would pass on a \
         rule written after the fact"
    );

    let _state = open_with_store(&repo, &config_dir(&tmp));

    assert!(repo.join(".turbogit").exists(), "the write happened");
    assert!(
        listed(&repo),
        "the rule is not in the clone's own exclude file"
    );
    let out = git(&repo, &["status", "--porcelain"]);
    assert!(
        !out.contains(".turbogit"),
        "the changes list still reports the app's directory: {out}"
    );
}

#[test]
fn opening_a_project_from_welcome_hides_it_too() {
    let tmp = TempDir::new().unwrap();
    let repo = temp_repo(tmp.path(), "repo");
    let cfg = config_dir(&tmp);

    // The Welcome-screen path: launch with nothing open, then open the folder.
    let mut state = AppState::launch_in(None, Some(cfg.clone()));
    state.open_project(&repo);
    state.save_durable_ui();

    assert!(listed(&repo), "open_project did not reach the listing");
    let out = git(&repo, &["status", "--porcelain"]);
    assert!(
        !out.contains(".turbogit"),
        "a repository opened from Welcome reports the store: {out}"
    );
}

#[test]
fn attaching_a_workspace_whose_top_is_one_repository_hides_it() {
    let tmp = TempDir::new().unwrap();
    let repo = temp_repo(tmp.path(), "repo");
    let cfg = config_dir(&tmp);

    let mut state = AppState::launch_in(None, Some(cfg.clone()));
    state.attach_workspace(&repo);
    state.save_durable_ui();

    assert!(
        listed(&repo),
        "attach_workspace registers its roots itself instead of through rescan, \
         which is the one path where the listing is easy to lose"
    );
    assert!(
        !git(&repo, &["status", "--porcelain"]).contains(".turbogit"),
        "the deep-scan path left the store in the changes list"
    );
}

#[test]
fn a_workspace_of_sibling_repositories_gets_no_rule_written_into_it() {
    let tmp = TempDir::new().unwrap();
    let alpha = temp_repo(tmp.path(), "alpha");
    let beta = temp_repo(tmp.path(), "beta");
    let workspace = tmp.path().to_path_buf();
    let cfg = config_dir(&tmp);

    let mut state = AppState::launch_in(None, Some(cfg.clone()));
    state.attach_workspace(&workspace);
    state.save_durable_ui();

    assert!(
        workspace.join(".turbogit").exists(),
        "and the store is where this app always puts it — at the workspace"
    );
    for repo in [&alpha, &beta] {
        assert!(
            !listed(repo),
            "a repository that cannot see the store (it sits above its work tree) \
             had its own git directory edited: {}",
            repo.display()
        );
    }
}
