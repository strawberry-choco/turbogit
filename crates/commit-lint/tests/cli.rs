use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn write_message(dir: &TempDir, name: &str, message: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, message).expect("write message file");
    path
}

fn lint(path: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_commit-lint"))
        .arg(path)
        .output()
        .expect("spawn commit-lint");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), stderr)
}

fn assert_rejected(path: &Path) {
    let (ok, stderr) = lint(path);
    assert!(!ok, "expected rejection for {}", path.display());
    assert!(
        !stderr.trim().is_empty(),
        "expected a parse error on stderr for {}",
        path.display()
    );
}

// Run from the crate directory, cocogitto discovers the repo root from cwd and loads its
// `cog.toml` — the plain cases exercise the repo settings; only
// `falls_back_to_default_settings_without_a_repository` (cwd = tempdir) is on the default path.
#[test]
fn valid_message_passes_silently() {
    let dir = TempDir::new().expect("tempdir");
    let (ok, stderr) = lint(&write_message(
        &dir,
        "valid.txt",
        "feat: add commit-lint tooling crate\n\nA body line.\n",
    ));
    assert!(ok);
    assert_eq!(stderr, "");
}

#[test]
fn wip_message_fails() {
    let dir = TempDir::new().expect("tempdir");
    assert_rejected(&write_message(&dir, "wip.txt", "wip\n"));
}

#[test]
fn missing_colon_fails() {
    let dir = TempDir::new().expect("tempdir");
    assert_rejected(&write_message(&dir, "nocolon.txt", "feat no colon here\n"));
}

#[test]
fn empty_subject_fails() {
    let dir = TempDir::new().expect("tempdir");
    assert_rejected(&write_message(&dir, "emptysub.txt", "feat:\n"));
}

// Outside any repository the settings loader cannot find a `cog.toml` and must
// fall back to defaults instead of panicking: defaults leave scopes unset, so a
// free-form scope still parses.
#[test]
fn falls_back_to_default_settings_without_a_repository() {
    let dir = TempDir::new().expect("tempdir");
    let message = write_message(&dir, "scoped.txt", "feat(any-free-scope): subject\n");
    let output = Command::new(env!("CARGO_BIN_EXE_commit-lint"))
        .arg(&message)
        .current_dir(dir.path())
        .output()
        .expect("spawn commit-lint");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let bad = write_message(&dir, "wip.txt", "wip\n");
    let output = Command::new(env!("CARGO_BIN_EXE_commit-lint"))
        .arg(&bad)
        .current_dir(dir.path())
        .output()
        .expect("spawn commit-lint");
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}
