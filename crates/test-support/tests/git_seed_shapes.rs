use std::path::{Path, PathBuf};

use test_support::git_seed::{
    git, repo_with_conflict, repo_with_feature_branch, repo_with_one_commit, repo_with_origin,
    two_repos,
};

fn out(repo: &Path, args: &[&str]) -> String {
    git(repo, args).trim().to_owned()
}

fn toplevel(repo: &Path) -> PathBuf {
    std::fs::canonicalize(out(repo, &["rev-parse", "--show-toplevel"])).expect("canonical toplevel")
}

fn ahead_behind(repo: &Path, left: &str, right: &str) -> String {
    out(
        repo,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{left}...{right}"),
        ],
    )
    .replace('\t', " ")
}

#[test]
fn the_plain_recipe_is_one_commit_on_main_and_has_no_remote() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = repo_with_one_commit(project.path(), "solo");

    assert_eq!(
        toplevel(&repo),
        std::fs::canonicalize(&repo).expect("canonical recipe path"),
        "the recipe must build the repository in the project dir it was handed"
    );
    assert_eq!(out(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(out(&repo, &["rev-list", "--count", "HEAD"]), "1");
    assert!(
        out(&repo, &["remote"]).is_empty(),
        "the plain recipe promises NO remote; git reports {:?}",
        out(&repo, &["remote"])
    );
    assert!(
        repo.join(".git").is_dir(),
        "it is a working repository, not a bare one"
    );
}

#[test]
fn the_two_repo_recipe_is_two_sibling_repositories_the_app_can_discover() {
    let project = tempfile::tempdir().expect("temp project dir");
    let (alpha, beta) = two_repos(project.path(), "alpha", "beta");

    for (label, repo) in [("alpha", &alpha), ("beta", &beta)] {
        assert_eq!(
            toplevel(repo),
            std::fs::canonicalize(repo).unwrap(),
            "{label} is its own repository"
        );
        assert_eq!(
            out(repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "main",
            "{label} is on main"
        );
        assert_eq!(
            out(repo, &["rev-list", "--count", "HEAD"]),
            "1",
            "{label} has one commit"
        );
        assert!(out(repo, &["remote"]).is_empty(), "{label} has no remote");
    }
    assert!(
        !alpha.starts_with(&beta) && !beta.starts_with(&alpha),
        "the two are siblings, so neither discovery swallows the other: {alpha:?} / {beta:?}"
    );

    #[cfg(feature = "harness")]
    {
        let state = turbogit_app::state::AppState::new(project.path().to_path_buf());
        let canon = |paths: Vec<&PathBuf>| -> Vec<PathBuf> {
            let mut p: Vec<PathBuf> = paths
                .into_iter()
                .map(std::fs::canonicalize)
                .collect::<Result<_, _>>()
                .expect("canonical root");
            p.sort();
            p
        };
        let found: Vec<&PathBuf> = state.multi.roots.iter().map(|r| &r.path).collect();
        assert_eq!(
            canon(found),
            canon(vec![&alpha, &beta]),
            "the recipe's promise is that discovery walks it into two roots"
        );
    }
}

#[test]
fn the_feature_recipe_is_origin_plus_main_plus_an_unpublished_feature() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = repo_with_feature_branch(project.path(), "dual");
    let branches = |args: &[&str]| {
        out(&repo, args)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        branches(&["branch", "--format=%(refname:short)"]),
        vec!["feature".to_string(), "main".to_string()],
        "both main and feature exist, and nothing else"
    );
    assert_eq!(
        out(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main",
        "HEAD is left on main, so a suite may check feature out itself"
    );
    assert_eq!(out(&repo, &["remote"]), "origin");
    assert_eq!(
        out(&repo, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main",
        "main is its own upstream, so ahead/behind is a known zero"
    );
    assert_eq!(ahead_behind(&repo, "origin/main", "main"), "0 0");
    assert_eq!(out(&repo, &["rev-list", "--count", "feature"]), "2");
    assert_eq!(
        out(&repo, &["diff", "--name-only", "main..feature"]),
        "feature.txt"
    );
    assert_eq!(
        branches(&["branch", "-r", "--format=%(refname:short)"]),
        vec!["origin/main".to_string()],
        "feature stays LOCAL: the recipe publishes main only, so a suite pushing \
         feature is exercising the push rather than a recipe that pre-empted it"
    );
}

#[test]
fn the_conflict_recipe_leaves_a_merge_in_progress_with_one_unmerged_path() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = repo_with_conflict(project.path(), "clash");

    assert_eq!(
        out(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main",
        "the merge was started from main, so ours/theirs are main/side in that order"
    );
    assert_eq!(
        out(&repo, &["rev-parse", "--verify", "-q", "MERGE_HEAD"]).len(),
        40,
        "MERGE_HEAD exists: the merge is still in progress"
    );
    assert_eq!(
        out(&repo, &["diff", "--name-only", "--diff-filter=U"]),
        "conf.txt"
    );
    let status = out(&repo, &["status", "--porcelain"]);
    assert!(
        status.contains("UU conf.txt"),
        "git status must report the path as both-unmerged; got {status:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("conf.txt")).expect("conf.txt on disk"),
        "<<<<<<< HEAD\nmain\n=======\nside\n>>>>>>> side\n",
        "the working copy carries git's own two-way conflict markers — ours \
         (HEAD) first, theirs last — the default `merge` style a resolver parses"
    );
    assert_eq!(
        out(&repo, &["ls-files", "-u", "conf.txt"])
            .lines()
            .map(|l| l.split_whitespace().nth(2).unwrap_or_default().to_owned())
            .collect::<Vec<_>>(),
        vec!["1".to_string(), "2".to_string(), "3".to_string()],
        "base, ours and theirs are all staged"
    );
}

#[test]
fn the_origin_recipe_still_yields_a_pushed_main_with_a_bare_remote() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = repo_with_origin(project.path(), "solo");
    let bare = project.path().join("solo.git");

    assert!(
        bare.join("HEAD").is_file(),
        "origin is bare, beside the work tree"
    );
    assert_eq!(
        out(&repo, &["remote", "get-url", "origin"]),
        bare.to_str().unwrap()
    );
    assert_eq!(
        out(&repo, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main"
    );
    assert_eq!(ahead_behind(&repo, "origin/main", "main"), "0 0");
    assert_eq!(
        out(&repo, &["rev-parse", "main"]),
        out(&bare, &["rev-parse", "main"])
    );
}
