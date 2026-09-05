//! Issue 31 — `tag_create` through the engine seam with the full `TagSpec`:
//! any commit as the target, annotated tags with a tagger identity, and GPG
//! signing. Headless integration tests over real git repositories (tempdir +
//! system `git`); the created tag objects are read back with git plumbing.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_domain::model::{TagSpec, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

fn exec() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
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

/// Append a line to `file.txt`, stage, commit; returns the new HEAD SHA.
fn commit(dir: &Path, msg: &str) -> String {
    let file = dir.join("file.txt");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{msg}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Fresh repo on `main` with two commits; returns `(guard, repo, [c1, c2])`.
fn repo_two_commits() -> (tempfile::TempDir, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    let c1 = commit(&repo, "one");
    let c2 = commit(&repo, "two");
    (tmp, repo, vec![c1, c2])
}

fn spec(name: &str) -> TagSpec {
    TagSpec {
        name: name.to_string(),
        ..Default::default()
    }
}

/// The raw tag object (`git cat-file tag <name>`) — annotated tags only.
fn tag_object(repo: &Path, name: &str) -> String {
    git(repo, &["cat-file", "tag", name])
}

#[test]
fn annotated_tag_on_a_non_head_commit_carries_tagger_message_and_target() {
    let (_tmp, repo, commits) = repo_two_commits();
    let mut s = spec("v0.9.0");
    s.target = Some(commits[0].clone());
    s.message = Some("Release 0.9.0".into());
    s.tagger = Some("Stink Ma <stink@turbogit.dev>".into());
    exec().tag_create(&repo, &s).unwrap();

    assert_eq!(git(&repo, &["tag", "-l"]).trim(), "v0.9.0");
    let obj = tag_object(&repo, "v0.9.0");
    assert!(
        obj.contains(&format!("object {}", commits[0])),
        "tag should point at the first commit, got:\n{obj}"
    );
    assert!(
        obj.contains("tagger Stink Ma <stink@turbogit.dev>"),
        "tagger identity should come from the spec, got:\n{obj}"
    );
    assert!(obj.contains("Release 0.9.0"), "message should be set");
}

#[test]
fn annotated_tag_without_a_tagger_uses_the_config_identity() {
    let (_tmp, repo, _commits) = repo_two_commits();
    let mut s = spec("v1");
    s.message = Some("hi".into());
    exec().tag_create(&repo, &s).unwrap();
    let obj = tag_object(&repo, "v1");
    assert!(
        obj.contains("tagger Test <test@example.com>"),
        "config identity should tag, got:\n{obj}"
    );
}

#[test]
fn lightweight_tag_on_a_target_peels_to_it() {
    let (_tmp, repo, commits) = repo_two_commits();
    let mut s = spec("old");
    s.target = Some(commits[0].clone());
    exec().tag_create(&repo, &s).unwrap();
    assert_eq!(
        git(&repo, &["rev-parse", "old^{}"]).trim(),
        commits[0],
        "a lightweight tag should point at the target commit"
    );
}

#[test]
fn lightweight_tag_defaults_to_head() {
    let (_tmp, repo, commits) = repo_two_commits();
    exec().tag_create(&repo, &spec("head-tag")).unwrap();
    assert_eq!(git(&repo, &["rev-parse", "head-tag"]).trim(), commits[1]);
}

#[test]
fn a_signed_tag_carries_a_signature_block() {
    let (_tmp, repo, _commits) = repo_two_commits();
    // A stub gpg program: git requires the `SIG_CREATED` status line on
    // stderr, then appends the program's stdout to the tag object — enough
    // to assert the `-s` flag reached git without provisioning a keychain.
    let stub = _tmp.path().join("fake-gpg.sh");
    std::fs::write(
        &stub,
        "#!/bin/sh\ncat > /dev/null\necho '[GNUPG:] SIG_CREATED ' >&2\necho '-----BEGIN PGP SIGNATURE-----'\necho fake\necho '-----END PGP SIGNATURE-----'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(&repo, &["config", "gpg.program", &stub.to_string_lossy()]);

    let mut s = spec("signed");
    s.message = Some("signed release".into());
    s.sign = true;
    exec().tag_create(&repo, &s).unwrap();
    let obj = tag_object(&repo, "signed");
    assert!(
        obj.contains("-----BEGIN PGP SIGNATURE-----"),
        "a signed tag should carry the signature block, got:\n{obj}"
    );

    let mut s = spec("unsigned");
    s.message = Some("plain release".into());
    exec().tag_create(&repo, &s).unwrap();
    assert!(
        !tag_object(&repo, "unsigned").contains("-----BEGIN PGP SIGNATURE-----"),
        "sign: false must not pass -s"
    );
}

#[test]
fn creating_a_duplicate_tag_fails() {
    let (_tmp, repo, _commits) = repo_two_commits();
    exec().tag_create(&repo, &spec("v1")).unwrap();
    assert!(exec().tag_create(&repo, &spec("v1")).is_err());
}
