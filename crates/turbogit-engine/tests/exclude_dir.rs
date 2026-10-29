use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::{GitExecutor, build_executor, cli::CliExecutor};

fn engine() -> std::sync::Arc<dyn GitExecutor> {
    build_executor(&VcsSettings::default())
}

struct Repo {
    path: PathBuf,
    _dir: TempDir,
}

fn temp_repo() -> Repo {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("proj");
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "base.txt"]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path, _dir: dir }
}

fn with_store(repo: &Path) -> PathBuf {
    let store = repo.join(".turbogit");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("ui.ron"), "tab: \"Commit\"\n").unwrap();
    store
}

fn exclude_file(repo: &Path) -> PathBuf {
    repo.join(".git").join("info").join("exclude")
}

fn status_text(repo: &Path) -> String {
    git(repo, &["status", "--porcelain"])
}

fn engine_paths(repo: &Path) -> Vec<PathBuf> {
    engine()
        .status(repo)
        .expect("status of a real repository")
        .changes
        .into_iter()
        .map(|c| c.path)
        .collect()
}

#[test]
fn a_listed_directory_stops_being_reported_as_untracked_work() {
    let repo = temp_repo();
    let store = with_store(&repo.path);
    assert!(
        status_text(&repo.path).contains(".turbogit/"),
        "the fixture must start in the state the product complains about: {}",
        status_text(&repo.path)
    );

    engine()
        .exclude_dir(&repo.path, &store)
        .expect("listing the store in a normal repository");

    assert!(
        !status_text(&repo.path).contains(".turbogit"),
        "still shown after being listed: {}",
        status_text(&repo.path)
    );
    assert!(
        !engine_paths(&repo.path)
            .iter()
            .any(|p| p.starts_with(".turbogit")),
        "the changes list is the surface that had the complaint, so the engine's \
         own answer is the one that must drop it: {:?}",
        engine_paths(&repo.path)
    );
}

#[test]
fn asking_twice_leaves_one_rule() {
    let repo = temp_repo();
    let store = with_store(&repo.path);
    let e = engine();
    e.exclude_dir(&repo.path, &store).unwrap();
    e.exclude_dir(&repo.path, &store).unwrap();
    e.exclude_dir(&repo.path, &store).unwrap();

    let text = std::fs::read_to_string(exclude_file(&repo.path)).unwrap();
    assert_eq!(
        text.lines().filter(|l| l.trim() == ".turbogit/").count(),
        1,
        "a rescan per open would otherwise stack the same rule until nobody can \
         read the file: {text}"
    );
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with("# Added by TurboGit"))
            .count(),
        1,
        "the explanatory heading is written with the rule, not on every ask: {text}"
    );
    assert!(
        !status_text(&repo.path).contains(".turbogit"),
        "and the third ask still left the answer: {}",
        status_text(&repo.path)
    );
}

#[test]
fn rules_the_repository_already_had_keep_working() {
    let repo = temp_repo();
    let store = with_store(&repo.path);
    std::fs::create_dir_all(repo.path.join("build")).unwrap();
    std::fs::write(repo.path.join("build").join("out.o"), "x").unwrap();
    std::fs::write(exclude_file(&repo.path), "build/").unwrap();

    engine()
        .exclude_dir(&repo.path, &store)
        .expect("listing beside existing rules");

    let text = std::fs::read_to_string(exclude_file(&repo.path)).unwrap();
    assert!(
        text.lines().any(|l| l.trim() == "build/"),
        "the repository's own rule was overwritten, not appended to: {text}"
    );
    assert!(
        text.contains("\n.turbogit/\n"),
        "the new rule is on its own line: {text:?}"
    );
    assert!(
        !status_text(&repo.path).contains("build"),
        "`build/` stopped being a rule once something was appended to it: {}",
        status_text(&repo.path)
    );
}

#[test]
fn a_linked_worktree_lands_in_the_common_dir_and_is_read_there() {
    let repo = temp_repo();
    let worktree = repo.path.parent().unwrap().join("wt");
    let worktree_str = worktree.to_str().unwrap().to_string();
    git(
        &repo.path,
        &["worktree", "add", "-q", "--detach", &worktree_str],
    );

    let store = with_store(&worktree);
    engine()
        .exclude_dir(&worktree, &store)
        .expect("listing from inside a linked worktree");

    let own = git(&worktree, &["rev-parse", "--absolute-git-dir"]);
    let own_dir = Path::new(own.trim());
    assert!(
        !own_dir.join("info").join("exclude").exists(),
        "the entry went to the worktree's own gitdir, which git never reads: {}",
        own_dir.display()
    );
    assert!(
        exclude_file(&repo.path).exists(),
        "the common directory's exclude file is the one git reads for every work \
         tree of the clone, so that is where the write belongs"
    );

    assert!(
        !status_text(&worktree).contains(".turbogit"),
        "the worktree that asked is the surface that had to stop showing it: {}",
        status_text(&worktree)
    );
}

#[test]
fn a_bare_repository_has_no_work_tree_to_hide_anything_in() {
    let dir = TempDir::new().unwrap();
    let bare = dir.path().join("bare.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q"]);

    let store = bare.join(".turbogit");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("ui.ron"), "tab: \"Commit\"\n").unwrap();

    let e = engine();
    assert!(
        e.exclude_dir(&bare, &store).is_err(),
        "a bare repository reports no untracked files, so writing its own \
         directory would be an edit the developer cannot explain"
    );
    let written = std::fs::read_to_string(bare.join("info").join("exclude")).unwrap_or_default();
    assert!(
        !written.contains(".turbogit/"),
        "the refusal still edited the bare directory: {written}"
    );
}

#[test]
fn the_pattern_is_a_basename_so_it_matches_at_any_depth() {
    let repo = temp_repo();
    let nested = repo.path.join("sub").join("deep");
    std::fs::create_dir_all(nested.join(".turbogit")).unwrap();
    std::fs::write(nested.join(".turbogit").join("ui.ron"), "tab: \"Log\"\n").unwrap();

    CliExecutor {
        settings: VcsSettings::default(),
    }
    .exclude_dir(&nested, &nested.join(".turbogit"))
    .expect("listing from a directory inside the work tree");

    let text = std::fs::read_to_string(exclude_file(&repo.path)).unwrap();
    assert!(
        text.lines().any(|l| l.trim() == ".turbogit/"),
        "the recorded rule is the basename, not a re-based path: {text}"
    );
    assert!(
        !status_text(&repo.path).contains(".turbogit"),
        "and the top level stops reporting the store that sits two directories in: {}",
        status_text(&repo.path)
    );
}
