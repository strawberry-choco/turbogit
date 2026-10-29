use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_app::state::AppState;

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

fn config_dir(tmp: &TempDir) -> PathBuf {
    let cfg = tmp.path().join("config");
    std::fs::create_dir_all(&cfg).unwrap();
    cfg
}

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
