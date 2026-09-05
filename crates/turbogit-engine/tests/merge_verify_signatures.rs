//! Issue 28 — the merge dialog's new option passes through to the CLI
//! adapter: `MergeOpts::verify_signatures` maps to `git merge
//! --verify-signatures`, which refuses to merge incoming commits that carry
//! no GPG signature.

use std::path::{Path, PathBuf};

use turbogit_domain::model::{MergeOpts, VcsSettings};
use turbogit_engine::GitExecutor as _;
use turbogit_engine::cli::CliExecutor;

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
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

/// Append `text` to `<dir>/file.txt`, stage, commit.
fn commit(dir: &Path, text: &str) {
    let file = dir.join("file.txt");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-q", "-m", text]);
}

/// Repo on `main` with a `feature` branch strictly ahead (unsigned commits —
/// the fixture configures no signing).
fn unsigned_fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base");
    run_git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "feature-1");
    run_git(&repo, &["checkout", "-q", "main"]);
    (tmp, repo)
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

#[test]
fn verify_signatures_refuses_unsigned_incoming_commits() {
    let (_tmp, repo) = unsigned_fixture();
    let opts = MergeOpts {
        verify_signatures: true,
        ..Default::default()
    };
    let err = engine()
        .merge(&repo, "feature", &opts)
        .expect_err("unsigned merge must be refused");
    assert!(
        err.to_string().to_lowercase().contains("signature"),
        "expected a signature refusal, got: {err}"
    );
    // The refusal left the branch untouched.
    assert_eq!(
        run_git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
}

#[test]
fn without_the_option_unsigned_commits_merge_fine() {
    let (_tmp, repo) = unsigned_fixture();
    engine()
        .merge(&repo, "feature", &MergeOpts::default())
        .expect("plain merge of unsigned commits");
}
