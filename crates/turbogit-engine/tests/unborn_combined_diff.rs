//! The combined `HEAD` diff read on a repository with no commits.
//!
//! The local-changes preview is pinned to `HEAD ↔ worktree`, and in a
//! repository nobody has committed in yet that revision does not exist. The
//! engine's answer there is a named refusal, `TgError::NoCommitsYet`, whose
//! display text is a sentence for the person who has just run `git init`.
//!
//! The alternative design, rejected rather than left undiscovered, was to put
//! the empty tree in HEAD's place and report every index-tracked path as a
//! whole addition. That makes the pane answer, and it makes it answer falsely:
//! nothing in the repository is being compared, because there is nothing to
//! compare against, and a reader shown "every line is new" cannot tell that
//! shape from a real first commit staged for the first time. Git's own text at
//! that command (`fatal: bad revision 'HEAD'`) is the other wrong answer, so
//! this suite pins the third: a refusal with a name and a readable sentence.
//!
//! Every fixture is a real temporary repository driven by real `git`, per
//! `AGENTS.md`, and every claim is cross-checked against git's own output
//! (`status --porcelain`, `diff`, `diff --cached`, `diff HEAD`) rather than
//! against an internal representation.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_domain::error::TgError;
use turbogit_domain::model::{DiffOpts, Patch, PatchLineKind, PatchOrigin, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;

struct Repo {
    path: PathBuf,
    /// Keeps the temp directory alive for the duration of the test.
    _dir: TempDir,
}

/// An initialized repository on an unborn branch: repo-local identity, no
/// commit, and therefore no HEAD to resolve.
fn unborn_repo(name: &str) -> Repo {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "core.autocrlf", "false"]);
    Repo { path, _dir: dir }
}

/// The same, plus one commit, so `git diff HEAD` has a HEAD to resolve against.
fn repo_with_commit(name: &str) -> Repo {
    let repo = unborn_repo(name);
    std::fs::write(repo.path.join("base.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "init"]);
    repo
}

fn cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn libgit2() -> Git2Executor {
    Git2Executor::new(cli())
}

/// The combined read the preview asks for: whole root, `HEAD` on the left.
fn combined_opts() -> DiffOpts {
    DiffOpts {
        left: Some("HEAD".to_owned()),
        ..DiffOpts::default()
    }
}

/// One file's read, the shape a per-file pane asks for.
fn combined_opts_for(path: &str) -> DiffOpts {
    DiffOpts {
        path: Some(PathBuf::from(path)),
        ..combined_opts()
    }
}

/// Run git expecting a non-zero exit, and hand back its stderr: the unborn
/// fixtures need to assert that the revision they refuse really is missing, and
/// `git_seed::git` asserts the opposite.
#[track_caller]
fn git_failing(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git on PATH");
    assert!(
        !out.status.success(),
        "git {args:?} in {} was expected to fail",
        dir.display()
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The empty tree of `dir`, derived by git itself. The suite uses it to show
/// what the refused baseline *would* have answered, which is the design this
/// file exists to keep from coming back.
#[track_caller]
fn empty_tree_oid(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["hash-object", "-t", "tree", "--stdin"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("git on PATH");
    assert!(out.status.success(), "git hash-object failed");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Where git would have put `oid` had anything written it as a loose object.
fn loose_object(repo: &Path, oid: &str) -> PathBuf {
    repo.join(".git/objects").join(&oid[..2]).join(&oid[2..])
}

/// The sentence a person sees. Held here rather than repeated per test,
/// because it is the deliverable: the refusal must stay a sentence someone can
/// act on, and not drift back into git's fatal text.
const NO_COMMITS_MESSAGE: &str =
    "this repository has no commits yet: there is no first commit to compare your changes against";

/// Assert the refusal is the named variant carrying that exact sentence. A test
/// that only matched on the text would pass for `TgError::Other`, which is the
/// one shape this design exists to avoid: a consumer cannot tell "no commits"
/// from any other failed read without re-reading prose.
#[track_caller]
fn assert_no_commits_refusal(err: &TgError) {
    assert!(
        matches!(err, TgError::NoCommitsYet { .. }),
        "expected TgError::NoCommitsYet, got {err}"
    );
    assert_eq!(err.to_string(), NO_COMMITS_MESSAGE);
}

/// The changed body of a patch, spelled the way the diff reads, built from the
/// value's own fields.
fn body(patch: &Patch) -> Vec<String> {
    patch
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.lines)
        .map(|l| {
            let prefix = match l.kind {
                PatchLineKind::Context => ' ',
                PatchLineKind::Added => '+',
                PatchLineKind::Removed => '-',
            };
            format!("{prefix}{}", l.text)
        })
        .collect()
}

/// The paths one patch reports, by their new side.
fn paths(patch: &Patch) -> Vec<String> {
    patch.files.iter().map(|f| f.new_path.clone()).collect()
}

// ----------------------------------------------------------- unborn HEAD ----

/// The ticket, reversed. A fresh repository with one file in the index does not
/// answer a `HEAD` read at all: it refuses, with the named variant and the
/// readable sentence, on the whole root and on one path, and whichever of the
/// two command arms the revision reaches through.
#[test]
fn combined_read_of_an_unborn_head_is_refused_with_a_readable_message() {
    let repo = unborn_repo("unborn-combined");
    std::fs::write(repo.path.join("f.txt"), "hello\nworld\n").unwrap();
    git(&repo.path, &["add", "f.txt"]);
    assert_eq!(
        git(&repo.path, &["diff", "--cached", "--name-only"]).trim(),
        "f.txt",
        "precondition: the index holds the file, so there IS content a \
         synthesized baseline would have reported"
    );
    // And HEAD is genuinely missing: this is git's own fatal text, the answer
    // the named refusal stands in for.
    let stderr = git_failing(&repo.path, &["diff", "HEAD", "--", "f.txt"]);
    assert!(
        stderr.contains("bad revision") || stderr.contains("unknown revision"),
        "git itself must refuse HEAD here, got {stderr}"
    );

    let err = cli()
        .diff_patch(&repo.path, &combined_opts())
        .expect_err("no commits means no baseline to diff against");
    assert_no_commits_refusal(&err);

    // The per-file pane asks the same question of the same missing revision.
    let scoped = cli()
        .diff_patch(&repo.path, &combined_opts_for("f.txt"))
        .expect_err("a path does not make HEAD exist");
    assert_eq!(scoped.to_string(), NO_COMMITS_MESSAGE);

    // Both command arms that name a revision route through the guard: the bare
    // left side and the explicit `left..right` range.
    let ranged = cli()
        .diff_patch(
            &repo.path,
            &DiffOpts {
                left: Some("HEAD".to_owned()),
                right: Some("main".to_owned()),
                ..DiffOpts::default()
            },
        )
        .expect_err("a range whose left side is HEAD is the same missing commit");
    assert_no_commits_refusal(&ranged);

    // The refusal derives nothing, so nothing lands in the developer's object
    // database: the shape the substituted read used to produce is unreachable.
    let sha1_empty_tree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    assert!(
        !loose_object(&repo.path, sha1_empty_tree).exists(),
        "a refused read writes no object"
    );

    // The contrast that makes the decision visible rather than assumed: naming
    // the empty tree outright still answers, so the refusal is about a caller
    // asking for HEAD, and the invented baseline is exactly the answer this
    // suite now keeps out of the app.
    let synthesized = cli()
        .diff_patch(
            &repo.path,
            &DiffOpts {
                left: Some(empty_tree_oid(&repo.path)),
                ..DiffOpts::default()
            },
        )
        .expect("the empty tree is a resolvable revision");
    assert_eq!(
        body(&synthesized),
        vec!["+hello".to_owned(), "+world".to_owned()],
        "and this is the false answer the pane no longer gets"
    );
}

/// The other state of the same root: nothing in the index at all. It is refused
/// for the same reason, not answered as an empty patch, because "no commits" is
/// a fact about the baseline and not about the size of the diff.
#[test]
fn combined_read_of_an_unborn_head_with_an_empty_index_is_refused_too() {
    let repo = unborn_repo("unborn-empty");
    std::fs::write(repo.path.join("scratch.txt"), "untracked\n").unwrap();
    git_failing(&repo.path, &["diff", "HEAD"]);
    assert_eq!(
        git(&repo.path, &["ls-files"]).trim(),
        "",
        "precondition: the index is empty, so the old design answered the \
         empty patch and this one must not be confused with it"
    );

    let err = cli()
        .diff_patch(&repo.path, &combined_opts())
        .expect_err("an unborn HEAD has no baseline either way");
    assert_no_commits_refusal(&err);
}

/// Only a read naming `HEAD` is refused. A caller naming a branch or a sha on
/// an unborn root still gets git's error, because that is a different missing
/// thing and folding it into the same variant would let a surface offer "make a
/// first commit" for a revision the user named themselves.
#[test]
fn combined_read_of_a_revision_that_is_not_head_keeps_git_own_error() {
    let repo = unborn_repo("unborn-other-rev");
    std::fs::write(repo.path.join("f.txt"), "hello\n").unwrap();
    git(&repo.path, &["add", "f.txt"]);

    let err = cli()
        .diff_patch(
            &repo.path,
            &DiffOpts {
                left: Some("main".to_owned()),
                ..DiffOpts::default()
            },
        )
        .expect_err("a branch with no commit does not exist");
    assert!(
        matches!(err, TgError::Cli { .. }),
        "git's own refusal, not the named one: {err}"
    );
    assert!(
        err.to_string().contains("bad revision") || err.to_string().contains("unknown revision"),
        "{err}"
    );
}

// ------------------------------------------------------- committed HEAD ----

/// The refusal must fire only where HEAD is absent. In a repository with a
/// commit, the same read is byte-for-byte `git diff HEAD`, unchanged by any of
/// this, and it reports removals, which no empty-tree baseline could produce.
#[test]
fn combined_read_with_a_commit_is_exactly_git_diff_head() {
    let repo = repo_with_commit("committed-combined");
    std::fs::write(repo.path.join("base.txt"), "one\nCHANGED\nthree\nfour\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);

    let patch = cli().diff_patch(&repo.path, &combined_opts()).unwrap();
    let text = patch.to_string();
    assert_eq!(
        text,
        git(&repo.path, &["diff", "HEAD", "--", "base.txt"]),
        "a committed HEAD read is git's own answer, unchanged by the unborn rule"
    );
    let lines = body(&patch);
    assert!(
        lines.contains(&"-two".to_owned()),
        "the read is against HEAD's content, not against no content: {lines:?}"
    );

    // The contrast, in the same repository: had the unborn rule fired, the
    // answer would have been the whole file as additions with no removals.
    let substituted = cli()
        .diff_patch(
            &repo.path,
            &DiffOpts {
                left: Some(empty_tree_oid(&repo.path)),
                ..DiffOpts::default()
            },
        )
        .unwrap();
    assert_ne!(
        substituted, patch,
        "and the two baselines are visibly different answers, so this test \
         cannot pass for the wrong reason"
    );
    assert!(!body(&substituted).iter().any(|l| l.starts_with('-')));
}

/// An untracked file stays out of the combined read, and reaches it the same way
/// it always did: intent-to-add. Nothing here changes the staging route; this
/// pins that the baseline git picks does not quietly start showing untracked
/// content. It is a committed fixture now, because on an unborn root the read
/// refuses before it can skip anything.
#[test]
fn combined_read_skips_an_untracked_file_until_intent_to_add() {
    let repo = repo_with_commit("committed-untracked");
    std::fs::write(repo.path.join("new.txt"), "brand new\n").unwrap();
    std::fs::write(repo.path.join("base.txt"), "one\ntwo\nthree\nedited\n").unwrap();

    let before = cli().diff_patch(&repo.path, &combined_opts()).unwrap();
    assert_eq!(
        paths(&before),
        vec!["base.txt".to_owned()],
        "git has no diff for a path it never held"
    );

    // The marker the staging route writes (`cli.rs` `add_intent_to_add`, reached
    // from `partial::stage_untracked_selection`).
    git(&repo.path, &["add", "-N", "--", "new.txt"]);
    let after = cli().diff_patch(&repo.path, &combined_opts()).unwrap();
    assert_eq!(
        paths(&after),
        vec!["base.txt".to_owned(), "new.txt".to_owned()],
        "intent-to-add is what admits the file to the read"
    );
    assert!(
        body(&after).contains(&"+brand new".to_owned()),
        "and it arrives as its own whole content: {after}"
    );
}

// ------------------------------------------------------------- backends ----

/// Both adapters answer identically, in both states. Today
/// `Git2Executor::diff_patch` is a one-line delegation to the CLI, so the two
/// cannot differ, and a green run here is the expected green rather than a test
/// that catches nothing. It is the pin against someone later giving libgit2 its
/// own `diff_patch` and its own idea of what an unborn HEAD means.
#[test]
fn combined_read_of_an_unborn_head_refuses_identically_across_both_adapters() {
    let for_cli = unborn_repo("unborn-parity-cli");
    let for_libgit2 = unborn_repo("unborn-parity-git2");
    for repo in [&for_cli, &for_libgit2] {
        std::fs::write(repo.path.join("f.txt"), "hello\nworld\n").unwrap();
        std::fs::write(repo.path.join("second.txt"), "second\n").unwrap();
        git(&repo.path, &["add", "."]);
    }

    let cli_err = cli()
        .diff_patch(&for_cli.path, &combined_opts())
        .expect_err("the CLI refuses");
    let git2_err = libgit2()
        .diff_patch(&for_libgit2.path, &combined_opts())
        .expect_err("libgit2 delegates, so it refuses too");
    assert_no_commits_refusal(&cli_err);
    assert_no_commits_refusal(&git2_err);

    // And where HEAD does exist, both still answer the same bytes.
    let committed = repo_with_commit("committed-parity");
    std::fs::write(committed.path.join("base.txt"), "one\nX\nthree\n").unwrap();
    let from_cli = cli().diff_patch(&committed.path, &combined_opts()).unwrap();
    let from_git2 = libgit2()
        .diff_patch(&committed.path, &combined_opts())
        .unwrap();
    assert_eq!(
        from_cli, from_git2,
        "the two backends must be indistinguishable to the pane"
    );
    assert!(
        body(&from_cli).contains(&"-two".to_owned()),
        "and the answer they agree on is git's real diff against HEAD: {:?}",
        body(&from_cli)
    );
}

// ---------------------------------------------------------- patch_bytes ----

/// The create-a-patch read of local changes is the same `git diff HEAD`, and
/// it now routes through the same named refusal. Before this, the unborn
/// branch showed the app's sentence in the pane and git's `fatal: bad
/// revision 'HEAD'` in the Create Patch dialog, which is one product decision
/// told two ways. The dialog's own wording is what a person reads: the
/// `create_patch_error` line is the refusal's sentence with a `Create patch: `
/// prefix, and the copy-to-clipboard verb toasts the sentence plain.
#[test]
fn patch_bytes_of_local_changes_refuses_an_unborn_root_with_the_same_sentence() {
    let repo = unborn_repo("unborn-patch-bytes");
    std::fs::write(repo.path.join("f.txt"), "hello\nworld\n").unwrap();
    git(&repo.path, &["add", "--", "f.txt"]);

    let err = cli()
        .patch_bytes(
            &repo.path,
            &PatchOrigin::LocalChanges { paths: Vec::new() },
            false,
        )
        .expect_err("there is no HEAD for a local-changes patch to diff against");
    assert_no_commits_refusal(&err);

    // The reverse direction asks the same question of the same revision, and
    // a committed repository still answers git's own bytes.
    let committed = repo_with_commit("committed-patch-bytes");
    std::fs::write(committed.path.join("base.txt"), "one\nX\nthree\n").unwrap();
    let bytes = cli()
        .patch_bytes(
            &committed.path,
            &PatchOrigin::LocalChanges { paths: Vec::new() },
            false,
        )
        .expect("a committed HEAD answers, unchanged by the guard");
    let text = String::from_utf8(bytes).expect("patch bytes are utf-8 here");
    assert!(
        text.contains("-two") && text.contains("+X"),
        "the committed answer is still git's own diff: {text}"
    );
}
