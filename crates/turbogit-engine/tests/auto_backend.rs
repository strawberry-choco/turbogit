//! Issue #26 — Settings, Auto backend: `GitBackend::Auto` routes reads to
//! libgit2 and falls back to the CLI for anything libgit2 cannot do. The
//! factory is the seam (ADR-0001): these tests drive `build_executor` and
//! assert the behavior contract on a real temporary repository.

use std::path::Path;

use turbogit_domain::model::{GitBackend, VcsSettings};
use turbogit_engine::build_executor;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    tmp
}

#[test]
fn auto_backend_serves_reads_and_falls_back_to_cli() {
    let tmp = fixture_repo();
    let repo = tmp.path().join("repo");
    let settings = VcsSettings {
        backend: GitBackend::Auto,
        ..VcsSettings::default()
    };
    let exec = build_executor(&settings);

    assert!(exec.is_repo(&repo), "Auto must serve the read path");
    assert_eq!(
        exec.current_branch(&repo).unwrap().as_deref(),
        Some("main"),
        "Auto must serve reads"
    );
    assert!(
        !exec.branches(&repo).unwrap().is_empty(),
        "Auto must serve libgit2-native reads"
    );

    // `run_raw` is CLI-only territory (git2 cannot spawn custom commands);
    // Auto must fall back to the CLI executor for it.
    let status = exec
        .run_raw(&repo, &["status".into(), "--porcelain".into()])
        .expect("Auto must fall back to the CLI for unsupported ops");
    assert!(status.is_empty(), "unexpected status output: {status:?}");
}

#[test]
fn auto_backend_matches_explicit_libgit2_on_reads() {
    let tmp = fixture_repo();
    let repo = tmp.path().join("repo");
    let exec = |backend| {
        build_executor(&VcsSettings {
            backend,
            ..VcsSettings::default()
        })
    };
    let auto = exec(GitBackend::Auto);
    let libgit2 = exec(GitBackend::Libgit2);

    assert_eq!(
        auto.current_branch(&repo).unwrap(),
        libgit2.current_branch(&repo).unwrap()
    );
    assert_eq!(
        auto.log(&repo, &turbogit_domain::model::LogOpts::default())
            .unwrap()
            .len(),
        libgit2
            .log(&repo, &turbogit_domain::model::LogOpts::default())
            .unwrap()
            .len()
    );
}
