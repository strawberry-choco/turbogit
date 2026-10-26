//! The engine lists a directory in the clone's own `info/exclude` — the one
//! ignore mechanism that is local, never committed, and never a tracked file.
//!
//! The product fact is that the directory stops being reported: every assertion
//! here reads `git status` or the engine's own [`GitExecutor::status`] answer,
//! because a line written somewhere git does not read would look like a success
//! in the file and a failure on screen. That gap is what the worktree case
//! below pins — git reads the **common** directory's exclude file, and a
//! linked worktree's own `info/exclude` is never consulted.
//!
//! Real repositories, real git: `git init`, `git worktree add`, and
//! `git status --porcelain` are the fixtures and the oracles.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::{GitExecutor, build_executor, cli::CliExecutor};

/// The engine production builds: the composed pair, so the git2 adapter's
/// delegation to the CLI is on the tested path.
fn engine() -> std::sync::Arc<dyn GitExecutor> {
    build_executor(&VcsSettings::default())
}

struct Repo {
    path: PathBuf,
    _dir: TempDir,
}

/// A temp repository with one commit, so `HEAD` exists and status is readable.
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

/// The store directory, with content: an empty directory is not something git
/// reports as untracked, so a fixture without a file inside would pass without
/// ever having asked the question.
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

/// Every change path the engine's own status answer reports, repo-relative.
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
    // No trailing newline, which is what a hand-edit leaves: the appended rule
    // would otherwise be glued to it and match nothing.
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

    // The per-worktree gitdir is where a naive implementation writes, and where
    // git does not look: measured against git 2.54, an `info/exclude` there
    // leaves the directory listed as untracked.
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
    // `git init --bare` ships the template's `info/exclude`, so the assertion is
    // about the rule, not about the file's existence.
    let written = std::fs::read_to_string(bare.join("info").join("exclude")).unwrap_or_default();
    assert!(
        !written.contains(".turbogit/"),
        "the refusal still edited the bare directory: {written}"
    );
}

#[test]
fn the_pattern_is_a_basename_so_it_matches_at_any_depth() {
    // The caller passes an absolute path inside a repository whose top level is
    // HIGHER — opening a subdirectory of a clone, which this app registers as a
    // root because git answers for it. A work-tree-relative pattern would need
    // the top level resolved and the path re-based; the basename rule matches
    // because git reads the store's *name*, not its location.
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
