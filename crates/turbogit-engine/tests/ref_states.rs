//! Issue 17 — Log UX upgrades, engine seam: ref decorations carry state —
//! a remote-tracking ref reports `Gone` when its upstream branch was
//! deleted on the remote, and tags report pushed vs local-only.

use std::path::Path;

use turbogit_domain::model::{GitRefKind, RefState, VcsSettings};
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
fn decorations_report_remote_gone_and_tag_push_state() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    git(&repo, &["tag", "v1.0"]);

    git(
        &repo,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(&repo, &["push", "-q", "origin", "v1.0"]);

    // A second tag that never left the machine, and an upstream deletion
    // the local clone has not pruned yet. The deletion happens *inside*
    // the bare remote (`git push :main` would prune the local
    // remote-tracking ref too, leaving nothing to mark gone).
    git(&repo, &["tag", "v2.0"]);
    git(&origin, &["update-ref", "-d", "refs/heads/main"]);

    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    let deco = exec.ref_decorations(&repo).expect("decorations");
    let mut flat: Vec<(GitRefKind, String, RefState)> = deco
        .into_iter()
        .flat_map(|(_, refs)| refs.into_iter().map(|r| (r.kind, r.name, r.state)))
        .collect();
    flat.sort_by(|a, b| a.1.cmp(&b.1));

    let remote_main = flat
        .iter()
        .find(|(k, n, _)| *k == GitRefKind::Remote && n == "origin/main")
        .unwrap_or_else(|| panic!("origin/main decoration missing: {flat:?}"));
    assert_eq!(remote_main.2, RefState::Gone, "upstream was deleted");

    let v1 = flat
        .iter()
        .find(|(k, n, _)| *k == GitRefKind::Tag && n == "v1.0")
        .expect("v1.0 decoration");
    assert_eq!(v1.2, RefState::Pushed);

    let v2 = flat
        .iter()
        .find(|(k, n, _)| *k == GitRefKind::Tag && n == "v2.0")
        .expect("v2.0 decoration");
    assert_eq!(v2.2, RefState::LocalOnly);

    // The local branch itself carries no special state.
    let main = flat
        .iter()
        .find(|(k, n, _)| *k == GitRefKind::Branch && n == "main")
        .expect("main decoration");
    assert_eq!(main.2, RefState::Default);
}
