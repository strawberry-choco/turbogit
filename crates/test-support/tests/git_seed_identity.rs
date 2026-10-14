//! Where a test repository's commit identity comes from — the rule the runner
//! owns, in the one place that can state it for all of them.
//!
//! A sibling of `git_seed_shapes` rather than a fifth test inside it: that
//! suite's doc scopes it to what each *recipe* promises, and identity is a
//! property of the runner every recipe inherits.
//!
//! Every assertion here reads git's own answer back with `git log`. Asserting
//! that four variables were *passed* would test the builder against itself,
//! which is the tautology the shape suite's doc warns against; what has to be
//! caught is git disagreeing with the pin.

use std::path::{Path, PathBuf};
use std::process::Command;

use test_support::git_seed::{GitEnv, GitIdent, git, git_with_env};

/// `%an|%ae|%cn|%ce` of `HEAD`, git's own account of who authored it and who
/// committed it — the only thing this file is allowed to assert.
fn identities(repo: &Path) -> String {
    git(repo, &["log", "-1", "--format=%an|%ae|%cn|%ce"])
        .trim()
        .to_owned()
}

/// A repository with no `user.*` configured on it, holding one staged file: the
/// state every fixture in the tree starts from, and the state two suites could
/// not get past on a machine with no global identity.
fn headless_repo(name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let repo = tmp.path().join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "x\n").expect("stage a file");
    git(&repo, &["add", "."]);
    (tmp, repo)
}

/// git's own configuration suppression, applied to a spawned command and
/// nowhere else. The owner's runners inherit this process's environment by
/// contract, and mutating that from a test would be process-global and racy
/// across the binary's parallel threads (and would need `unsafe` on edition
/// 2024). These variables are also not owner behaviour — a fixture is not a
/// hermeticity harness — so they are added here, on the one command that has to
/// run as a machine with no identity of its own.
fn suppress_git_config(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/nonexistent/git-seed-identity-config");
}

#[test]
fn the_pinned_door_commits_a_repository_that_has_no_identity_of_its_own() {
    let (_tmp, repo) = headless_repo("pinned");

    // The four variables git reads for an author and a committer outrank every
    // configuration level, so the machine's global identity cannot reach this
    // commit — and if it somehow did, the assertion below is what would say so.
    // The sequence is exactly what two suites in this tree build: `app`'s
    // activity log pinned a committer under a name git does not read (the
    // misspelling the ratchet at the bottom of this file names), and `ui`'s
    // discard confirmation pinned an author alone. On a machine with no global
    // identity both died on this commit in fixture construction, before a test
    // could run.
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

    // Half the row: git's own answer on a machine that has no identity of its
    // own. A raw spawn rather than the owner's builder, because a pinned value
    // cannot be un-pinned and this row's whole subject is the command that
    // carries no identity at all. What it does take is git's own configuration
    // suppression, added on the command — the owner's runners inherit the process
    // environment, and mutating that from a test would be process-global and racy
    // across the binary's parallel threads.
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

    // The other half: the same complaint has to reach a suite that used the
    // unpinned door, which cannot carry the suppression above (inheriting the
    // process environment is its whole contract). What it must do is report git's
    // stderr, and keep the `PATH` claim for the spawn that just succeeded. A
    // repository with no commits is the one failure a machine of any
    // configuration agrees on.
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

    // The row no single repository `user.*` config can express, and the reason
    // the owner's identity value has one field per role: an engine suite's
    // branch-tip assertions read the author's name and the committer's time, and
    // collapsing the two would make its committer-time assertion assert nothing.
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

    // Dates are pinned only where a suite asserts on a commit's time, and they
    // ride on the same value as the identity — the two contexts whose menu rows
    // would otherwise reorder between runs on a same-second commit.
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

/// The misspelling is unspellable in the call grammar already — a caller fills
/// in a name and an email, never a variable name — so this only pins that it is
/// gone from the tree as *text* too.
///
/// It is a text read, and that weakness is accepted on purpose: it breaks if the
/// string moves, which is the same weakness the repo's port-discipline ratchet
/// already carries. One assertion is what it is worth.
#[test]
fn no_file_in_the_test_tree_spells_the_misspelled_variable() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.join("../..");
    let this = std::fs::canonicalize(manifest.join("tests/git_seed_identity.rs"))
        .expect("this file's own path");
    // Every `crates/*/tests` directory plus the root's own `tests/`, read at run
    // time rather than through a hand-kept file list: the claim is "nowhere", and
    // a list of the files checked when the ratchet was written stops covering new
    // ones silently.
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
        // Canonical, so this file's own path and the walked one are the same
        // string: `crates/test-support/../../crates/…` is the same file.
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

/// Read as text by the ratchet above, so it cannot be spelled inside a
/// `format!` without the needle itself appearing.
const TYPO: &str = concat!("GIT_AUTHOR_", "COMMITTER_");
