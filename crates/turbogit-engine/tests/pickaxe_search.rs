use test_support::git_seed::git;
use turbogit_domain::model::LogOpts;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;

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

    let all = exec.log(&repo, &LogOpts::default()).expect("unscoped log");
    assert_eq!(all.len(), 2);

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
