//! The pull guards, ported from the egui `pull_guards` suite: a rebase pull
//! refuses a protected branch, and a fleet pull integrates without
//! rewriting.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::git;
use turbogit_domain::model::{UpdateMethod, VcsSettings};

mod support;

fn behind_repo(project: &Project, name: &str) -> (PathBuf, PathBuf) {
    let repo = support::seeded_repo(project.path(), name);
    let bare = project.path().join(format!("{name}.git"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["init", "-q", "--bare", "-b", "main", &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    let other = project.path().join(format!("{name}-other"));
    git(
        project.path(),
        &["clone", "-q", &bare_s, other.to_str().unwrap()],
    );
    git(&other, &["config", "user.email", "t@t"]);
    git(&other, &["config", "user.name", "t"]);
    support::commit_into(&other, "remote.txt", "remote work");
    git(&other, &["push", "-q", "origin", "main"]);
    (repo, bare)
}

fn settings(method: UpdateMethod, protect_main: bool) -> VcsSettings {
    VcsSettings {
        update_method: method,
        protected_branch_patterns: if protect_main {
            vec!["main".to_string()]
        } else {
            Vec::new()
        },
        ..VcsSettings::default()
    }
}

fn branches_with(project: &Project, repo: &Path, settings: VcsSettings) -> Mounted {
    let mut app = for_roots_seeded(
        project.path(),
        std::slice::from_ref(&repo.to_path_buf()),
        move |s| {
            s.settings = settings.clone();
        },
    );
    app.first_render().unwrap();
    app.press("id:tab:Branches").unwrap();
    app.wait_for("id:branches:tree").unwrap();
    app
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn a_rebase_pull_refuses_a_protected_branch() {
    let project = Project::new();
    let (repo, _bare) = behind_repo(&project, "alpha");
    let before = head(&repo);
    let mut app = branches_with(&project, &repo, settings(UpdateMethod::Rebase, true));
    app.press("id:branches:pull").unwrap();
    app.wait_until("the refusal is stated", Duration::from_secs(10), |screen| {
        screen.texts().iter().any(|t| t.contains("protected"))
    })
    .unwrap();
    assert_eq!(head(&repo), before, "the protected branch never moved");
    assert!(
        app.screen().texts().iter().any(|t| t.contains("protected"))
            || app.screen().texts().iter().any(|t| t.contains("refus")),
        "the refusal is stated; painted={:?}",
        app.screen().texts()
    );
}

#[test]
fn a_merge_pull_integrates_without_rewriting() {
    let project = Project::new();
    let (repo, bare) = behind_repo(&project, "alpha");
    support::commit_into(&repo, "local.txt", "local work");
    let before = head(&repo);
    let mut app = branches_with(&project, &repo, settings(UpdateMethod::Merge, false));
    app.press("id:branches:pull").unwrap();
    for _ in 0..600 {
        if head(&repo) != before {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let after = head(&repo);
    assert_ne!(after, before, "the pull moved HEAD");
    assert_eq!(
        git(&repo, &["rev-parse", "origin/main"]).trim(),
        git(&bare, &["rev-parse", "refs/heads/main"]).trim(),
        "the remote ref tracks the remote tip"
    );
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", &after]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "a merge commit carries two parents, so history was not rewritten: {parents}"
    );
}
