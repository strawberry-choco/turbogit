use std::path::{Path, PathBuf};

use test_support::git_seed::{git, repo_with_one_commit, repo_with_origin};
use turbogit_domain::model::{MultiRootManager, RootId, VcsSettings};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::multi_root::{build_root, register};
use turbogit_services::sync_service::outgoing_per_root;

fn append_commit(dir: &Path, msg: &str) -> String {
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
    git(dir, &["commit", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn repo_ahead_of_origin() -> (tempfile::TempDir, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().to_path_buf();
    let repo = repo_with_origin(&project, "repo");
    let c1 = git(&repo, &["rev-parse", "main"]).trim().to_string();

    let c2 = append_commit(&repo, "c2");
    let c3 = append_commit(&repo, "c3");

    (tmp, repo, vec![c1, c2, c3])
}

#[test]
fn outgoing_commits_lists_local_ahead_shas_in_log_order() {
    let (_tmp, repo, shas) = repo_ahead_of_origin();
    let expected = vec![shas[2].clone(), shas[1].clone()];

    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let got = engine
        .outgoing_commits(&repo, "main", "origin/main")
        .expect("outgoing_commits");
    assert_eq!(got, expected);

    let raw = git(&repo, &["log", "--format=%H", "@{u}..HEAD"]);
    let via_git: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    assert_eq!(via_git, expected);
}

#[test]
fn outgoing_per_root_returns_one_list_per_root() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let mut mgr = MultiRootManager::default();

    let (_tmp1, repo1, shas1) = repo_ahead_of_origin();
    let (_tmp2, repo2, shas2) = repo_ahead_of_origin();
    register(&mut mgr, build_root(&engine, &repo1).expect("root 1"));
    register(&mut mgr, build_root(&engine, &repo2).expect("root 2"));

    let results = outgoing_per_root(&engine, &mgr);

    assert_eq!(results.len(), 2, "one entry per registered root");
    assert_eq!(results[0].0, RootId(repo1.clone().into()));
    assert_eq!(results[1].0, RootId(repo2.clone().into()));

    let out1 = results[0].1.as_ref().expect("root 1 result is Ok");
    let out2 = results[1].1.as_ref().expect("root 2 result is Ok");
    assert_eq!(out1, &vec![shas1[2].clone(), shas1[1].clone()]);
    assert_eq!(out2, &vec![shas2[2].clone(), shas2[1].clone()]);
}

#[test]
fn outgoing_per_root_yields_empty_for_root_without_upstream() {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let mut mgr = MultiRootManager::default();

    let (_tmp_good, good_repo, shas) = repo_ahead_of_origin();

    let tmp_solo = tempfile::tempdir().expect("tempdir");
    let solo_repo = repo_with_one_commit(tmp_solo.path(), "solo");

    register(
        &mut mgr,
        build_root(&engine, &good_repo).expect("good root"),
    );
    register(
        &mut mgr,
        build_root(&engine, &solo_repo).expect("solo root"),
    );

    let results = outgoing_per_root(&engine, &mgr);
    assert_eq!(results.len(), 2);

    let mut by_id = std::collections::HashMap::new();
    for (id, result) in results {
        by_id.insert(id, result);
    }

    let good = by_id
        .get(&RootId(good_repo.clone().into()))
        .expect("good root present")
        .as_ref()
        .expect("good root result is Ok");
    assert_eq!(good, &vec![shas[2].clone(), shas[1].clone()]);

    let solo = by_id
        .get(&RootId(solo_repo.clone().into()))
        .expect("solo root present")
        .as_ref()
        .expect("missing upstream must be graceful Ok, not Err");
    assert!(
        solo.is_empty(),
        "root without upstream must yield an empty outgoing list"
    );
}
