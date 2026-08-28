//! Golden-output tests around the CLI executor (`src/engine/cli.rs`) — Phase L0
//! of `docs/library-migration-execution-plan.md`.
//!
//! Every test builds a controlled fixture repository with real `git` commands
//! and pins the EXACT parsed output of [`CliExecutor`] for one read operation.
//! These tests are the parity oracle for the upcoming engine migration: a
//! replacement backend must reproduce these values to pass.
//!
//! Determinism: fixed identities, fixed `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE`
//! epochs, pure-LF file contents, repo-local config overriding any global
//! `core.autocrlf` / `diff.noprefix`. No snapshot framework — plain asserts.
//!
//! Behavior quirks pinned here on purpose (migration must preserve them):
//! - Conflicted (`u`) entries always parse to `staged: false, unstaged: false`
//!   and their path is taken as the LAST whitespace token of the line.
//! - `map_xy` prefers the index letter, falls back to the worktree letter,
//!   maps typechange `T` (and anything unknown) to `Modified`.
//! - `log` parses `%at` once and stores it in `author.time`, `committer.time`
//!   AND `Commit.time` — committer time is never read independently.
//! - `log` messages are `trim_end()`ed: git's trailing newline (and any
//!   trailing body whitespace) disappears.
//! - `ahead_behind` swaps rev-list's `<behind>\t<ahead>` columns into
//!   `(ahead, behind)`.

use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_domain::model::{ChangeStatus, Commit, DiffOpts, LogOpts, RootId, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

// --------------------------------------------------------- fixture helpers --

/// Fixed commit epochs (git internal `@<unix> <offset>` date format).
const DATE_1: &str = "@1112911993 +0000";
const DATE_2: &str = "@1234567890 +0000";
const DATE_3: &str = "@1700000000 +0000";
const TIME_1: i64 = 1_112_911_993;
const TIME_2: i64 = 1_234_567_890;
const TIME_3: i64 = 1_700_000_000;

/// The executor under test with default settings (git resolved from PATH).
fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Like [`run_git`] but pins author AND committer dates separately, so
/// fixture commits carry deterministic, assertion-friendly epochs.
fn run_git_dates(dir: &Path, args: &[&str], author_date: &str, committer_date: &str) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", author_date)
        .env("GIT_COMMITTER_DATE", committer_date)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// [`run_git_dates`] with author == committer date.
fn run_git_at(dir: &Path, args: &[&str], date: &str) -> String {
    run_git_dates(dir, args, date, date)
}

/// Run `git <args>` expecting FAILURE (e.g. a conflicting merge); returns
/// stdout + stderr combined so the caller can sanity-check why it failed.
/// (`git merge` reports its CONFLICT lines on stdout.)
fn run_git_failing(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(!out.status.success(), "git {args:?} unexpectedly succeeded");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Repo-local config that makes fixtures deterministic regardless of the
/// developer's global git config (identity, EOL translation, diff prefixes).
fn configure_identity(repo: &Path) {
    run_git(repo, &["config", "user.email", "golden@example.com"]);
    run_git(repo, &["config", "user.name", "Golden Author"]);
    run_git(repo, &["config", "core.autocrlf", "false"]);
    run_git(repo, &["config", "diff.noprefix", "false"]);
}

/// Write `content` to `<repo>/<name>`, stage and commit it at `date`;
/// returns the new HEAD SHA.
fn commit_file(repo: &Path, name: &str, content: &str, msg: &str, date: &str) -> String {
    std::fs::write(repo.join(name), content).expect("write fixture file");
    run_git(repo, &["add", "--", name]);
    run_git_at(repo, &["commit", "-q", "-m", msg], date);
    run_git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Fresh repository on `main` with one base commit (`base.txt = "base\n"` at
/// [`DATE_1`]). The caller keeps the returned [`tempfile::TempDir`] alive for
/// the duration of the test.
fn temp_repo(name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-q", "-b", "main"]);
    configure_identity(&repo);
    commit_file(&repo, "base.txt", "base\n", "base commit", DATE_1);
    (tmp, repo)
}

/// Full SHAs of `commits`, in order.
fn ids(commits: &[Commit]) -> Vec<String> {
    commits.iter().map(|c| c.id.clone()).collect()
}

/// Assert `diff` is git's canonical unified diff for `path`: the four header
/// lines (`diff --git`, `index …`, `---`, `+++`) followed by EXACTLY `body`
/// (hunk header + patch text). The `index` line carries blob hashes, so only
/// its shape is pinned; every other line is byte-exact.
#[track_caller]
fn assert_unified_diff(diff: &str, path: &str, body: &[&str]) {
    let lines: Vec<&str> = diff.lines().collect();
    assert_eq!(
        lines.len(),
        body.len() + 4,
        "unexpected diff shape:\n{diff}"
    );
    assert_eq!(lines[0], format!("diff --git a/{path} b/{path}"));
    assert!(
        lines[1].starts_with("index ") && lines[1].ends_with(" 100644"),
        "unexpected index line: {}",
        lines[1]
    );
    assert_eq!(lines[2], format!("--- a/{path}"));
    assert_eq!(lines[3], format!("+++ b/{path}"));
    assert_eq!(&lines[4..], body, "patch body mismatch:\n{diff}");
}

// ------------------------------------------------------------ status tests --

#[test]
fn engine_golden_status_clean_repo_is_empty() {
    let (_tmp, repo) = temp_repo("clean");
    let st = engine().status(&repo).expect("status on clean repo");
    assert!(st.changes.is_empty(), "got: {:?}", st.changes);
    assert!(st.conflicted.is_empty());
}

#[test]
fn engine_golden_status_parses_staged_add() {
    let (_tmp, repo) = temp_repo("staged-add");
    std::fs::write(repo.join("fresh.txt"), "fresh\n").expect("write");
    run_git(&repo, &["add", "fresh.txt"]);

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("fresh.txt"));
    assert_eq!(c.status, ChangeStatus::Added);
    // Porcelain v2 `1 A.`: index letter set, worktree letter '.'.
    assert!(c.staged);
    assert!(!c.unstaged);
    assert_eq!(c.orig_path, None);
    assert!(c.chunks.is_empty());
    assert!(st.conflicted.is_empty());
}

#[test]
fn engine_golden_status_parses_unstaged_modify() {
    let (_tmp, repo) = temp_repo("unstaged-modify");
    std::fs::write(repo.join("base.txt"), "changed\n").expect("write");

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("base.txt"));
    assert_eq!(c.status, ChangeStatus::Modified);
    // Porcelain v2 `1 .M`: index matches HEAD, worktree differs.
    assert!(!c.staged);
    assert!(c.unstaged);
    assert_eq!(c.orig_path, None);
}

#[test]
fn engine_golden_status_parses_untracked_file() {
    let (_tmp, repo) = temp_repo("untracked");
    std::fs::write(repo.join("ghost.txt"), "boo\n").expect("write");

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("ghost.txt"));
    assert_eq!(c.status, ChangeStatus::Unversioned);
    // `? ` entries carry no staging information at all.
    assert!(!c.staged);
    assert!(!c.unstaged);
    assert_eq!(c.orig_path, None);
}

#[test]
fn engine_golden_status_parses_deleted_file() {
    let (_tmp, repo) = temp_repo("deleted");
    std::fs::remove_file(repo.join("base.txt")).expect("remove tracked file");

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("base.txt"));
    assert_eq!(c.status, ChangeStatus::Deleted);
    // Porcelain v2 `1 .D`: deletion happened in the worktree only.
    assert!(!c.staged);
    assert!(c.unstaged);
}

#[test]
fn engine_golden_status_parses_renamed_file_with_orig_path() {
    let (_tmp, repo) = temp_repo("renamed");
    run_git(&repo, &["mv", "base.txt", "renamed.txt"]);

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    // Porcelain v2 `2 R.` entry: `path` is the NEW path, `orig_path` the old.
    assert_eq!(c.path, PathBuf::from("renamed.txt"));
    assert_eq!(c.orig_path, Some(PathBuf::from("base.txt")));
    assert_eq!(c.status, ChangeStatus::Renamed);
    assert!(c.staged);
    assert!(!c.unstaged);
}

#[test]
fn engine_golden_status_maps_merge_conflict_to_conflicted() {
    let (_tmp, repo) = temp_repo("conflict");
    // Both branches edit the same line of the same file.
    run_git(&repo, &["branch", "feature"]);

    std::fs::write(repo.join("clash.txt"), "main side\n").expect("write main side");
    run_git(&repo, &["add", "clash.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "main change"], DATE_2);

    run_git(&repo, &["switch", "feature"]);
    std::fs::write(repo.join("clash.txt"), "feature side\n").expect("write feature side");
    run_git(&repo, &["add", "clash.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "feature change"], DATE_3);

    run_git(&repo, &["switch", "main"]);
    let err = run_git_failing(&repo, &["merge", "feature"]);
    assert!(
        err.contains("CONFLICT"),
        "fixture must produce a merge conflict, got: {err}"
    );

    let st = engine().status(&repo).expect("status during conflict");
    // The `u ` porcelain entry replaces any `1 `/`2 ` entry for the path.
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("clash.txt"));
    assert_eq!(c.status, ChangeStatus::Conflicted);
    // QUIRK: the parser ignores the `u` line's XY pair entirely — conflicts
    // are neither staged nor unstaged in the parsed model.
    assert!(!c.staged);
    assert!(!c.unstaged);
    assert_eq!(c.orig_path, None);
    assert_eq!(st.conflicted, vec![PathBuf::from("clash.txt")]);
}

// -------------------------------------------------------------- log tests --

#[test]
fn engine_golden_log_pins_linear_history_fields() {
    let (_tmp, repo) = temp_repo("log-linear");
    let c1 = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let c2 = commit_file(&repo, "two.txt", "two\n", "second subject", DATE_2);
    let c3 = commit_file(
        &repo,
        "three.txt",
        "three\n",
        "subject three\n\nbody alpha\nbody beta",
        DATE_3,
    );

    let commits = engine().log(&repo, &LogOpts::default()).expect("log");
    assert_eq!(commits.len(), 3, "whole history, newest first");
    assert_eq!(ids(&commits), vec![c3.clone(), c2.clone(), c1.clone()]);

    // Parent counts and ids for a strictly linear history.
    assert!(commits[2].parents.is_empty(), "root commit has no parents");
    assert_eq!(commits[1].parents, vec![c1]);
    assert_eq!(commits[0].parents, vec![c2]);

    // Identity comes from the repo-local config for BOTH signatures.
    for c in &commits {
        assert_eq!(c.author.name, "Golden Author");
        assert_eq!(c.author.email, "golden@example.com");
        assert_eq!(c.committer.name, "Golden Author");
        assert_eq!(c.committer.email, "golden@example.com");
        // The commit is tied to the exact root path handed to the engine.
        assert_eq!(c.root, RootId(repo.clone().into()));
    }

    // Epochs come straight from the fixed GIT_*_DATE fixtures.
    assert_eq!(commits[0].time, TIME_3);
    assert_eq!(commits[0].author.time, TIME_3);
    assert_eq!(commits[0].committer.time, TIME_3);
    assert_eq!(commits[1].time, TIME_2);
    assert_eq!(commits[2].time, TIME_1);

    // Messages: subject-only rows and subject+body rows (the raw `%B` with
    // git's trailing newline trimmed off).
    assert_eq!(
        commits[0].message, "subject three\n\nbody alpha\nbody beta",
        "%B must carry subject AND body"
    );
    assert_eq!(commits[1].message, "second subject");
    assert_eq!(commits[2].message, "base commit");
}

#[test]
fn engine_golden_log_branch_filter_and_max_count() {
    let (_tmp, repo) = temp_repo("log-filter");
    let c1 = run_git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let c2 = commit_file(&repo, "two.txt", "two\n", "second", DATE_2);
    let c3 = commit_file(&repo, "three.txt", "three\n", "third", DATE_3);

    // Branch filter walks ONLY the named ref's history.
    run_git(&repo, &["branch", "topic", &c1]);
    let topic = engine()
        .log(
            &repo,
            &LogOpts {
                branch: Some("topic".to_string()),
                ..LogOpts::default()
            },
        )
        .expect("log topic");
    assert_eq!(ids(&topic), vec![c1.clone()]);

    // max_count caps the newest-first walk of HEAD.
    let two = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(2),
                ..LogOpts::default()
            },
        )
        .expect("log -n2");
    assert_eq!(ids(&two), vec![c3, c2]);

    // Both options combine (`git log -n1 topic`).
    let combo = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(1),
                branch: Some("topic".to_string()),
                ..LogOpts::default()
            },
        )
        .expect("log -n1 topic");
    assert_eq!(ids(&combo), vec![c1]);
}

#[test]
fn engine_golden_log_committer_time_mirrors_author_time() {
    let (_tmp, repo) = temp_repo("log-committer-time");
    // A commit whose author and committer epochs deliberately diverge.
    std::fs::write(repo.join("split.txt"), "split\n").expect("write");
    run_git(&repo, &["add", "split.txt"]);
    run_git_dates(
        &repo,
        &["commit", "-q", "-m", "split dates"],
        DATE_1,
        DATE_3,
    );

    let commits = engine().log(&repo, &LogOpts::default()).expect("log");
    let top = &commits[0];
    assert_eq!(top.message, "split dates");
    assert_eq!(top.author.time, TIME_1);
    // QUIRK: cli.rs parses `%at` once and stamps it into all three time
    // fields — the real committer epoch (TIME_3) is never observed. A
    // migrated engine must decide whether to preserve or fix this; the
    // oracle currently REQUIRES the mirroring.
    assert_eq!(top.committer.time, TIME_1);
    assert_eq!(top.time, TIME_1);
}

// -------------------------------------------------------------- diff tests --

#[test]
fn engine_golden_diff_unstaged_modification_exact_text() {
    let (_tmp, repo) = temp_repo("diff-modify");
    const BASE: &str = "alpha\nbravo\ncharlie\ndelta\necho\n";
    std::fs::write(repo.join("words.txt"), BASE).expect("write");
    run_git(&repo, &["add", "words.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "words"], DATE_1);

    // Controlled single-line edit in the middle of the file.
    std::fs::write(repo.join("words.txt"), BASE.replace("bravo", "BRAVO")).expect("rewrite");

    let diff = engine()
        .diff(&repo, &DiffOpts::default())
        .expect("working-tree diff");
    assert_unified_diff(
        &diff,
        "words.txt",
        &[
            "@@ -1,5 +1,5 @@",
            " alpha",
            "-bravo",
            "+BRAVO",
            " charlie",
            " delta",
            " echo",
        ],
    );
}

#[test]
fn engine_golden_diff_staged_vs_worktree_semantics() {
    let (_tmp, repo) = temp_repo("diff-staged");
    std::fs::write(repo.join("words.txt"), "one\ntwo\nthree\n").expect("write");
    run_git(&repo, &["add", "words.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "words"], DATE_1);

    // Stage v2, then diverge the worktree to v3: index holds "TWO", the
    // worktree holds "TWO!".
    std::fs::write(repo.join("words.txt"), "one\nTWO\nthree\n").expect("stage v2");
    run_git(&repo, &["add", "words.txt"]);
    std::fs::write(repo.join("words.txt"), "one\nTWO!\nthree\n").expect("worktree v3");

    // Default opts → `git diff` (worktree vs INDEX).
    let wt = engine().diff(&repo, &DiffOpts::default()).expect("wt diff");
    assert_unified_diff(
        &wt,
        "words.txt",
        &["@@ -1,3 +1,3 @@", " one", "-TWO", "+TWO!", " three"],
    );

    // staged: true → `git diff --cached` (INDEX vs HEAD).
    let staged = engine()
        .diff(
            &repo,
            &DiffOpts {
                staged: true,
                ..DiffOpts::default()
            },
        )
        .expect("cached diff");
    assert_unified_diff(
        &staged,
        "words.txt",
        &["@@ -1,3 +1,3 @@", " one", "-two", "+TWO", " three"],
    );
}

#[test]
fn engine_golden_diff_ignore_whitespace_flag_mapping() {
    let (_tmp, repo) = temp_repo("diff-ws");
    std::fs::write(repo.join("words.txt"), "alpha\nbravo\ncharlie\n").expect("write");
    run_git(&repo, &["add", "words.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "words"], DATE_1);

    // Whitespace-only edit: visible by default, invisible under the flag
    // (mapped to `git diff --ignore-all-space`).
    std::fs::write(repo.join("words.txt"), "alpha\nb r a v o\ncharlie\n").expect("whitespace edit");

    let plain = engine()
        .diff(&repo, &DiffOpts::default())
        .expect("plain diff");
    assert!(
        plain.contains("-bravo") && plain.contains("+b r a v o"),
        "whitespace edit must show without the flag:\n{plain}"
    );

    let ignored = engine()
        .diff(
            &repo,
            &DiffOpts {
                ignore_whitespace: true,
                ..DiffOpts::default()
            },
        )
        .expect("ws-insensitive diff");
    assert_eq!(
        ignored, "",
        "--ignore-all-space must silence a whitespace-only edit"
    );
}

// ------------------------------------------------------- ahead/behind test --

#[test]
fn engine_golden_ahead_behind_counts_on_clone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("origin-src");
    std::fs::create_dir_all(&src).expect("src dir");
    run_git(&src, &["init", "-q", "-b", "main"]);
    configure_identity(&src);
    commit_file(&src, "base.txt", "base\n", "base", DATE_1);

    // Clone the fixture repo to a second path; the clone tracks origin/main.
    let clone = tmp.path().join("clone");
    run_git(
        tmp.path(),
        &["clone", "-q", src.to_str().expect("utf8 src"), "clone"],
    );
    configure_identity(&clone);

    let ex = engine();
    assert_eq!(
        ex.ahead_behind(&clone, "main", "origin/main")
            .expect("ahead/behind in sync"),
        (0, 0)
    );

    // One purely local commit → ahead 1, behind 0. This direction-check pins
    // the column swap: rev-list prints "<behind>\t<ahead>" but the seam
    // returns (ahead, behind).
    let _c2 = commit_file(&clone, "ahead.txt", "ahead\n", "local ahead", DATE_2);
    assert_eq!(
        ex.ahead_behind(&clone, "main", "origin/main")
            .expect("ahead/behind diverged local"),
        (1, 0)
    );

    // The upstream moves independently; after fetching, the clone is both
    // ahead and behind by one.
    let _c3 = commit_file(&src, "upstream.txt", "upstream\n", "upstream move", DATE_3);
    run_git(&clone, &["fetch", "-q", "origin"]);
    assert_eq!(
        ex.ahead_behind(&clone, "main", "origin/main")
            .expect("ahead/behind diverged both"),
        (1, 1)
    );
}

// --------------------------------------------- libgit2 backend parity (L2) --

/// The libgit2 backend must reproduce the CLI's `current_branch` value on a
/// fresh repository with an unborn HEAD: `git symbolic-ref --short HEAD`
/// succeeds there (the branch name exists before any commit does), so
/// `is_repo` — defined as `current_branch().is_ok()` — must treat a freshly
/// initialized repository as a repository. Otherwise root discovery misses
/// it and opening a fresh repo never enters the shell (issue #10 flow).
#[test]
fn git2_backend_current_branch_matches_cli_on_unborn_head() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("fresh");
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-q", "-b", "main"]);

    let cli = engine();
    let git2 = turbogit_engine::git2_exec::Git2Executor::new(CliExecutor {
        settings: VcsSettings::default(),
    });

    let expected = cli.current_branch(&repo).expect("CLI on unborn HEAD");
    assert_eq!(expected.as_deref(), Some("main"));
    assert_eq!(
        git2.current_branch(&repo).expect("git2 on unborn HEAD"),
        expected,
        "libgit2 backend must match the CLI on an unborn HEAD"
    );
    assert!(
        git2.is_repo(&repo),
        "a fresh repo must be detected as a repo"
    );
}
