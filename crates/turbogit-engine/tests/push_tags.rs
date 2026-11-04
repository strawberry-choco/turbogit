//! The push dialog's tags combo has two entries because git has two flags:
//! `--tags` and `--follow-tags`. A boolean could not tell them apart, and a
//! three-state control whose third state sent the same argv as its second was a
//! control that lied. This pins the three modes as three different commands
//! against real git, where the difference is observable: a lightweight tag on
//! the pushed tip rides `--tags` only, an annotated one rides both.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::build_executor;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::PushTags;

fn executors() -> Vec<(&'static str, Arc<dyn GitExecutor>)> {
    vec![
        (
            "cli",
            Arc::new(CliExecutor {
                settings: VcsSettings::default(),
            }),
        ),
        ("composed", build_executor(&VcsSettings::default())),
    ]
}

/// `main`, one commit ahead of its own bare `origin`, with a lightweight and an
/// annotated tag both on the commit being pushed.
fn tagged_and_ahead(project: &Path, name: &str) -> (PathBuf, PathBuf) {
    let work = repo_with_origin(project, name);
    commit(&work, "next.txt", "next\n", "next work");
    git(&work, &["tag", "light"]);
    git(&work, &["tag", "-a", "-m", "note", "annotated"]);
    let remote = project.join(format!("{name}.git"));
    (work, remote)
}

fn remote_tags(remote: &Path) -> Vec<String> {
    git(remote, &["tag", "-l"])
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// One mode, every adapter, each on its own fixture: a push consumes the state,
/// so a second adapter measuring the same mode must not inherit the first's.
fn tags_after(mode: PushTags) -> Vec<Vec<String>> {
    let tmp = tempfile::tempdir().expect("tempdir");
    executors()
        .into_iter()
        .map(|(label, exec)| {
            let (work, remote) = tagged_and_ahead(tmp.path(), &format!("repo-{label}"));
            let before = git(&remote, &["rev-parse", "main"]).trim().to_string();
            exec.push(&work, "origin", "main", false, mode, false, false, None)
                .unwrap_or_else(|e| panic!("{label} adapter refused the push: {e}"));
            let after = git(&remote, &["rev-parse", "main"]).trim().to_string();
            assert_ne!(
                after, before,
                "the {label} adapter must still push the branch"
            );
            remote_tags(&remote)
        })
        .collect()
}

#[test]
fn all_is_tags_and_current_branch_is_follow_tags() {
    for (mode, want) in [
        (
            PushTags::All,
            vec!["annotated".to_string(), "light".to_string()],
        ),
        (PushTags::Follow, vec!["annotated".to_string()]),
        (PushTags::None, vec![]),
    ] {
        for got in tags_after(mode) {
            assert_eq!(got, want, "{mode:?} must answer the same on every adapter");
        }
    }
}
