//! Issue 13 — custom command passthrough through the engine seam.
//!
//! The user types an arbitrary git command into the bulk-operations modal;
//! the port needs a raw-args escape hatch so the CLI adapter can run it per
//! repo. Headless tests over real temporary repositories.

use turbogit_domain::error::TgError;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

/// Fresh initialized repo on `main` with local identity configured.
fn temp_repo(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(tag);
    std::fs::create_dir_all(&repo).expect("repo dir");
    for args in [
        ["init", "-q", "-b", "main"].as_slice(),
        ["config", "user.email", "test@example.com"].as_slice(),
        ["config", "user.name", "Test"].as_slice(),
    ] {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("spawning git");
        assert!(out.status.success(), "git {args:?} failed");
    }
    (tmp, repo)
}

fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

#[test]
fn run_raw_runs_arbitrary_git_args_in_the_root_and_returns_stdout() {
    let (_tmp, repo) = temp_repo("raw-ok");
    let exe = executor();

    let out = exe
        .run_raw(&repo, &["config".to_string(), "user.email".to_string()])
        .expect("raw config read succeeds");
    assert_eq!(out.trim(), "test@example.com", "stdout is returned");

    // A mutating command lands exactly like the typed command would.
    exe.run_raw(
        &repo,
        &[
            "config".to_string(),
            "turbogit.test".to_string(),
            "custom".to_string(),
        ],
    )
    .expect("raw config write succeeds");
    assert_eq!(
        exe.config_get(&repo, "turbogit.test").unwrap().as_deref(),
        Some("custom")
    );
}

#[test]
fn a_failing_raw_command_surfaces_the_git_stderr_as_a_cli_error() {
    let (_tmp, repo) = temp_repo("raw-fail");
    let exe = executor();

    let err = exe
        .run_raw(&repo, &["definitely-not-a-git-command".to_string()])
        .expect_err("an unknown subcommand fails");
    match &err {
        TgError::Cli { stderr, .. } => {
            assert!(
                stderr.contains("definitely-not-a-git-command"),
                "git stderr surfaced: {stderr:?}"
            );
        }
        other => panic!("expected TgError::Cli, got {other:?}"),
    }
}

#[test]
fn run_raw_honors_the_root_as_working_directory() {
    let (_tmp, repo) = temp_repo("raw-cwd");
    std::fs::write(repo.join("tracked.txt"), "hello\n").unwrap();
    for args in [
        ["add", "."].as_slice(),
        ["commit", "-q", "-m", "c1"].as_slice(),
    ] {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
    }

    let exe = executor();
    let out = exe
        .run_raw(
            &repo,
            &["rev-parse".to_string(), "--show-toplevel".to_string()],
        )
        .expect("raw rev-parse succeeds");
    let expected = std::fs::canonicalize(&repo).unwrap();
    let reported = std::path::PathBuf::from(out.trim());
    assert_eq!(
        std::fs::canonicalize(reported).unwrap(),
        expected,
        "the command ran inside the root"
    );
}
