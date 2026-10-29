use std::path::{Path, PathBuf};
use std::process::Command;

use test_support::git_seed::{GitEnv, GitIdent, git, git_with_env};

fn identities(repo: &Path) -> String {
    git(repo, &["log", "-1", "--format=%an|%ae|%cn|%ce"])
        .trim()
        .to_owned()
}

fn headless_repo(name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let repo = tmp.path().join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "x\n").expect("stage a file");
    git(&repo, &["add", "."]);
    (tmp, repo)
}

fn suppress_git_config(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/nonexistent/git-seed-identity-config");
}

#[test]
fn the_pinned_door_commits_a_repository_that_has_no_identity_of_its_own() {
    let (_tmp, repo) = headless_repo("pinned");

    // The four variables git reads for an author and a committer outrank every
    // configuration level, so the machine's global identity cannot reach this
    // commit, and if it somehow did, the assertion below is what would say so.
    git_with_env(&repo, &["commit", "-q", "-m", "init"], &GitEnv::fixture());

    assert_eq!(
        identities(&repo),
        "t|t@t|t|t@t",
        "the first commit of a repository with no identity of its own commits as \
         the owner's shared pin, author and committer both"
    );
}

#[test]
fn the_repo_config_door_has_no_identity_to_borrow_and_reports_the_rule_it_broke() {
    let (_tmp, repo) = headless_repo("unpinned");

    let mut cmd = Command::new("git");
    cmd.args(["commit", "-q", "-m", "init"]).current_dir(&repo);
    suppress_git_config(&mut cmd);
    let out = cmd.output().expect("git must be on PATH");

    assert!(
        !out.status.success(),
        "a commit with no identity configured anywhere cannot succeed: this row \
         exists to pin that the two doors are genuinely different"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("identity unknown"),
        "git's complaint must name the identity rule — there is no other rule this \
         commit could break; git said: {stderr}"
    );

    let panic = std::panic::catch_unwind(|| git(&repo, &["rev-parse", "--verify", "HEAD"]))
        .expect_err("HEAD does not resolve in a repository with no commits");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("the panic carries a message");
    assert!(
        message.contains("fatal:"),
        "the unpinned door's failure reports what git said: {message}"
    );
    assert!(
        !message.contains("git must be on PATH"),
        "`git must be on PATH` is the spawn's claim, and the spawn is exactly what \
         succeeded; a failure that reports it names a rule nothing broke: {message}"
    );
}

#[test]
fn one_call_can_pin_an_author_its_committer_does_not_share() {
    let (_tmp, repo) = headless_repo("split");

    let env = GitEnv {
        author: GitIdent {
            name: "ada",
            email: "ada@example",
        },
        committer: GitIdent {
            name: "comm",
            email: "comm@example",
        },
        author_date: None,
        committer_date: None,
    };
    git_with_env(&repo, &["commit", "-q", "-m", "init"], &env);

    assert_eq!(
        identities(&repo),
        "ada|ada@example|comm|comm@example",
        "git must report each role's own bytes, not one value worn twice"
    );
}

#[test]
fn a_pinned_date_reaches_the_commit_git_records() {
    let (_tmp, repo) = headless_repo("dated");

    let mut env = GitEnv::fixture();
    env.author_date = Some("@100 +0000".to_string());
    env.committer_date = Some("@100 +0000".to_string());
    git_with_env(&repo, &["commit", "-q", "-m", "init"], &env);

    assert_eq!(
        git(&repo, &["log", "-1", "--format=%at|%ct"]).trim(),
        "100|100",
        "git must record both pinned timestamps, not the wall clock"
    );
}

#[test]
fn no_file_in_the_test_tree_spells_the_misspelled_variable() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.join("../..");
    let this = std::fs::canonicalize(manifest.join("tests/git_seed_identity.rs"))
        .expect("this file's own path");
    let mut bases: Vec<std::path::PathBuf> = vec![workspace.join("tests")];
    bases.extend(
        std::fs::read_dir(workspace.join("crates"))
            .expect("crates/")
            .flatten()
            .map(|e| e.path().join("tests"))
            .filter(|p| p.is_dir()),
    );

    let mut found: Vec<String> = Vec::new();
    for base in bases {
        let base = std::fs::canonicalize(&base).expect("tests dir canonical");
        for (rel, source) in test_support::srcscan::sources_under(&base) {
            let path = base.join(&rel);
            if path == this {
                continue;
            }
            for (no, line) in source.lines().enumerate() {
                if line.contains(TYPO) {
                    found.push(format!("{}:{}", path.display(), no + 1));
                }
            }
        }
    }

    assert!(
        found.is_empty(),
        "`{TYPO}` is not a git variable — git reads `GIT_COMMITTER_*` — and a \
         fixture that spells it commits under whatever the machine has configured. \
         Found it in: {found:?}"
    );
}

const TYPO: &str = concat!("GIT_AUTHOR_", "COMMITTER_");
