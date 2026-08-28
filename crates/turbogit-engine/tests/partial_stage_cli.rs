//! Partial staging engine lane (spec R2) — real-CLI integration tests.
//!
//! Drives [`turbogit_engine::cli::CliExecutor`] against temporary git
//! repositories and asserts end-state index/worktree state through git
//! itself (`git diff --cached`, `git diff`, `git status --porcelain`) —
//! never through internal representations. Covers the spec's real-CLI
//! layer: stage one hunk of two, reverse-unstage a hunk, intent-to-add
//! partial staging of an untracked file.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::{ApplyDirection, GitExecutor};

// ---------------------------------------------------------------- helpers --

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Repo {
    path: PathBuf,
    /// Keeps the temp directory alive for the duration of the test.
    _dir: TempDir,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly.
fn temp_repo(name: &str) -> Repo {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path, _dir: dir }
}

fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// 20-line base file. Edits to `bravo` (line 2) and `quebec` (line 17) are
/// separated by well over twice the default diff context, so git reports
/// them as two independent hunks.
const BASE: &str = concat!(
    "alpha\n",
    "bravo\n",
    "charlie\n",
    "delta\n",
    "echo\n",
    "foxtrot\n",
    "golf\n",
    "hotel\n",
    "india\n",
    "juliet\n",
    "kilo\n",
    "lima\n",
    "mike\n",
    "november\n",
    "oscar\n",
    "papa\n",
    "quebec\n",
    "romeo\n",
    "sierra\n",
    "tango\n",
);

/// Hand-written patch covering only the first hunk (the `bravo` edit),
/// preserving git's original hunk-header shape (ADR-0013).
const HUNK_ONE_PATCH: &str = concat!(
    "diff --git a/words.txt b/words.txt\n",
    "--- a/words.txt\n",
    "+++ b/words.txt\n",
    "@@ -1,5 +1,5 @@\n",
    " alpha\n",
    "-bravo\n",
    "+BRAVO\n",
    " charlie\n",
    " delta\n",
    " echo\n",
);

// ------------------------------------------------------------------ tests --

#[test]
fn stage_one_hunk_of_two() {
    let repo = temp_repo("stage-one-hunk");
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();

    let ex = executor();
    ex.apply_patch_to_index(&repo.path, HUNK_ONE_PATCH, ApplyDirection::Forward)
        .unwrap();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("-bravo") && staged.contains("+BRAVO"),
        "first hunk should be staged:\n{staged}"
    );
    assert!(
        !staged.contains("QUEBEC"),
        "second hunk must not be staged:\n{staged}"
    );

    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("-quebec") && unstaged.contains("+QUEBEC"),
        "second hunk should remain unstaged:\n{unstaged}"
    );
    assert!(
        !unstaged.contains("BRAVO"),
        "first hunk must stay out of the unstaged diff:\n{unstaged}"
    );

    let status = git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.lines().any(|line| line == "MM words.txt"),
        "file should be partially staged (MM):\n{status}"
    );
}

#[test]
fn reverse_unstage_a_hunk() {
    let repo = temp_repo("reverse-unstage-hunk");
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    // Stage the whole-file change (both hunks)...
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
    git(&repo.path, &["add", "words.txt"]);

    // ...then unstage only the first hunk by reverse-applying its patch.
    let ex = executor();
    ex.apply_patch_to_index(&repo.path, HUNK_ONE_PATCH, ApplyDirection::Reverse)
        .unwrap();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("-quebec") && staged.contains("+QUEBEC"),
        "second hunk should remain staged:\n{staged}"
    );
    assert!(
        !staged.contains("BRAVO"),
        "first hunk should have left the index:\n{staged}"
    );

    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("-bravo") && unstaged.contains("+BRAVO"),
        "first hunk should be back in the unstaged diff:\n{unstaged}"
    );
    assert!(
        !unstaged.contains("QUEBEC"),
        "second hunk must stay out of the unstaged diff:\n{unstaged}"
    );

    let status = git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.lines().any(|line| line == "MM words.txt"),
        "file should be partially staged (MM):\n{status}"
    );
}

#[test]
fn intent_to_add_partial_staging_of_untracked_file() {
    let repo = temp_repo("intent-to-add-partial");
    // Untracked file with six lines; only the first three will be staged.
    std::fs::write(
        repo.path.join("new.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\n",
    )
    .unwrap();

    let ex = executor();
    ex.add_intent_to_add(&repo.path, &[PathBuf::from("new.txt")])
        .unwrap();
    let creation_patch = concat!(
        "diff --git a/new.txt b/new.txt\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/new.txt\n",
        "@@ -0,0 +1,3 @@\n",
        "+one\n",
        "+two\n",
        "+three\n",
    );
    ex.apply_patch_to_index(&repo.path, creation_patch, ApplyDirection::Forward)
        .unwrap();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("+one") && staged.contains("+two") && staged.contains("+three"),
        "first three lines should be staged:\n{staged}"
    );
    assert!(
        !staged.contains("+four") && !staged.contains("+five") && !staged.contains("+six"),
        "remaining lines must not be staged:\n{staged}"
    );

    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("+four") && unstaged.contains("+five") && unstaged.contains("+six"),
        "remaining lines should stay unstaged:\n{unstaged}"
    );

    let status = git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.lines().any(|line| line == "AM new.txt"),
        "untracked file should show as partially staged (AM):\n{status}"
    );
}
