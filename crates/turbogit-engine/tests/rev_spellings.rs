//! The rev spellings a caller may hand `LogOpts::branch`, measured against both
//! adapters. The composed executor answers a single-rev read in-process
//! (`git2_exec.rs`), so a spelling that only works under the CLI would be a
//! production bug that no CLI-only suite can see.

use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::git2_exec::Git2Executor;
use turbogit_engine_api::GitExecutor;

fn cli() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn backends() -> Vec<(&'static str, Box<dyn GitExecutor>)> {
    vec![
        ("cli", Box::new(cli())),
        ("git2", Box::new(Git2Executor::new(cli()))),
    ]
}

fn read(exec: &dyn GitExecutor, repo: &Path, rev: &str) -> turbogit_domain::error::TgResult<usize> {
    exec.log(
        repo,
        &LogOpts {
            branch: Some(rev.to_string()),
            ..Default::default()
        },
    )
    .map(|commits| commits.len())
}

/// `main`, `origin/main`, plus the pushed-then-deleted `solo`, plus one
/// annotated and one lightweight tag.
fn seeded() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let work: PathBuf = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.email", "t@t"]);
    git(&work, &["config", "user.name", "t"]);
    for msg in ["one", "two"] {
        git(&work, &["commit", "-q", "--allow-empty", "-m", msg]);
    }
    let bare = tmp.path().join("origin.git");
    git(
        tmp.path(),
        &["init", "-q", "--bare", "-b", "main", "origin.git"],
    );
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&work, &["branch", "solo"]);
    git(&work, &["push", "-q", "origin", "solo"]);
    git(&work, &["branch", "-D", "solo"]);
    git(&work, &["tag", "-a", "-m", "annotated release", "v1"]);
    git(&work, &["tag", "v2"]);
    (tmp, work)
}

#[test]
fn a_remote_tracking_spelling_resolves_with_no_local_twin() {
    let (_tmp, work) = seeded();
    for (backend, exec) in backends() {
        assert_eq!(
            read(exec.as_ref(), &work, "origin/solo").ok(),
            Some(2),
            "{backend}: refs/remotes/origin/solo is the only solo that exists"
        );
    }
}

#[test]
fn a_bare_remote_name_is_not_a_rev_under_either_adapter() {
    let (_tmp, work) = seeded();
    for (backend, exec) in backends() {
        assert!(
            read(exec.as_ref(), &work, "solo").is_err(),
            "{backend}: git's checkout DWIM never applies to a revision argument, \
             so a listing row must send the qualified spelling"
        );
    }
}

#[test]
fn annotated_and_lightweight_tags_resolve_under_either_adapter() {
    let (_tmp, work) = seeded();
    for (backend, exec) in backends() {
        for tag in ["v1", "v2"] {
            assert_eq!(
                read(exec.as_ref(), &work, tag).ok(),
                Some(2),
                "{backend}: {tag} must reach the walk without an explicit peel"
            );
        }
    }
}

#[test]
fn a_local_name_resolves_under_either_adapter() {
    let (_tmp, work) = seeded();
    for (backend, exec) in backends() {
        assert_eq!(
            read(exec.as_ref(), &work, "main").ok(),
            Some(2),
            "{backend}: the name is the rev for a local branch"
        );
    }
}
