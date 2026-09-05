//! Issue 17 — Log UX upgrades, engine seam: `LogOpts::pickaxe` drives
//! `git log -S` so commit search can cover code changes — a commit matches
//! when the occurrence count of the string in the tracked content changed
//! there, even if its message, hash, and author do not mention it.

use std::path::Path;

use turbogit_domain::model::LogOpts;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;

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

#[test]
fn pickaxe_scopes_the_log_to_commits_changing_the_string() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);

    std::fs::write(repo.join("a.txt"), "the granular flag\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add config"]);
    let touching = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    std::fs::write(repo.join("b.txt"), "unrelated\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add other"]);
    let untouched = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };

    // Unscoped: both commits.
    let all = exec.log(&repo, &LogOpts::default()).expect("unscoped log");
    assert_eq!(all.len(), 2);

    // Pickaxe "granular": only the commit whose content changed the count.
    let hits = exec
        .log(
            &repo,
            &LogOpts {
                pickaxe: Some("granular".into()),
                ..Default::default()
            },
        )
        .expect("pickaxe log");
    let ids: Vec<&str> = hits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec![touching.as_str()], "only the touching commit");
    assert!(!ids.contains(&untouched.as_str()));
}
