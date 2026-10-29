use test_support::git_seed::git;
use turbogit_domain::model::{GitRefKind, RefState, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;

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

    let main = flat
        .iter()
        .find(|(k, n, _)| *k == GitRefKind::Branch && n == "main")
        .expect("main decoration");
    assert_eq!(main.2, RefState::Default);
}

#[test]
fn unreachable_remote_leaves_refs_unmarked_and_returns_promptly() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
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
        &["remote", "add", "origin", "http://127.0.0.1:1/unreachable"],
    );

    let started = std::time::Instant::now();
    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    let deco = exec.ref_decorations(&repo).expect("decorations");

    let mut seen_tag = None;
    for (_, refs) in &deco {
        if let Some(r) = refs.iter().find(|r| r.kind == GitRefKind::Tag) {
            seen_tag = Some(r);
        }
    }
    let tag = seen_tag.expect("v1.0 tag decoration");
    assert_eq!(
        tag.state,
        RefState::Default,
        "an unreachable remote must leave refs unmarked"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "offline ref decoration must return promptly, took {:?}",
        started.elapsed()
    );
}
