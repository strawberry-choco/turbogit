use std::path::Path;

use test_support::git_seed::{git, pin_eol_lf, repo_with_one_commit};

fn persisted_eol(repo: &Path) -> String {
    git(repo, &["config", "core.autocrlf"]).trim().to_owned()
}

#[test]
fn the_entry_pins_the_repository_it_is_given() {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let repo = tmp.path().join("handbuilt");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);

    pin_eol_lf(&repo);

    assert_eq!(
        persisted_eol(&repo),
        "false",
        "the entry writes the policy into the repository's own config, so git \
         reports LF-normalising text even on a machine whose system config turns \
         CRLF on"
    );
}

#[test]
fn a_recipe_repository_inherits_the_lf_policy() {
    let tmp = tempfile::tempdir().expect("temp project dir");

    let repo = repo_with_one_commit(tmp.path(), "eol");

    assert_eq!(
        persisted_eol(&repo),
        "false",
        "a suite adopting the seed recipe is deterministic on any machine without \
         calling the entry itself"
    );
}
