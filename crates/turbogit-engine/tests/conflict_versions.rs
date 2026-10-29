use std::path::{Path, PathBuf};
use std::process::Command;

use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;

fn git_merge_that_must_conflict(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        !status.status.success(),
        "the fixture merge should conflict, and it did not"
    );
}

fn exec() -> std::sync::Arc<dyn GitExecutor> {
    std::sync::Arc::new(turbogit_engine::cli::CliExecutor {
        settings: VcsSettings::default(),
    })
}

fn conflicting_repo(
    ours: &str,
    theirs: &str,
    ancestor: Option<&str>,
) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);

    match ancestor {
        Some(base) => {
            std::fs::write(repo.join("merged.txt"), base).unwrap();
            git(&repo, &["add", "merged.txt"]);
            git(&repo, &["commit", "-q", "-m", "ancestor"]);
        }
        None => {
            std::fs::write(repo.join("other.txt"), "x\n").unwrap();
            git(&repo, &["add", "other.txt"]);
            git(&repo, &["commit", "-q", "-m", "unrelated"]);
        }
    }

    git(&repo, &["switch", "-c", "theirs"]);
    std::fs::write(repo.join("merged.txt"), theirs).unwrap();
    git(&repo, &["add", "merged.txt"]);
    git(&repo, &["commit", "-q", "-m", "theirs"]);

    git(&repo, &["switch", "-q", "main"]);
    std::fs::write(repo.join("merged.txt"), ours).unwrap();
    git(&repo, &["add", "merged.txt"]);
    git(&repo, &["commit", "-q", "-m", "ours"]);

    git_merge_that_must_conflict(&repo, &["merge", "theirs"]);
    (tmp, repo)
}

#[test]
fn a_real_conflict_answers_three_distinct_sides() {
    let (_tmp, repo) = conflicting_repo("our side\n", "their side\n", Some("ancestor\n"));
    let versions = exec()
        .conflict_versions(&repo, Path::new("merged.txt"))
        .expect("a conflicted path answers its sides");
    assert_eq!(versions.base.as_deref(), Some("ancestor\n"));
    assert_eq!(versions.ours.as_deref(), Some("our side\n"));
    assert_eq!(versions.theirs.as_deref(), Some("their side\n"));
}

#[test]
fn an_add_add_conflict_has_no_base_at_all() {
    let (_tmp, repo) = conflicting_repo("ours\n", "theirs\n", None);
    let versions = exec()
        .conflict_versions(&repo, Path::new("merged.txt"))
        .expect("add/add is still a conflict with sides");
    assert_eq!(
        versions.base, None,
        "an add/add has no ancestor — not an empty one"
    );
    assert_eq!(versions.ours.as_deref(), Some("ours\n"));
    assert_eq!(versions.theirs.as_deref(), Some("theirs\n"));
}

#[test]
fn the_content_read_refuses_an_index_stage_spec_and_names_the_read_that_answers_it() {
    let (_tmp, repo) = conflicting_repo("our side\n", "their side\n", Some("ancestor\n"));
    let exec = exec();
    for rev in [":0", ":1", ":2", ":3"] {
        let err = exec
            .show_file(&repo, rev, Path::new("merged.txt"))
            .expect_err("a stage rev must not cross the port");
        assert!(
            format!("{err:?}").contains("conflict_versions")
                || format!("{err:?}").contains("index_file_bytes"),
            "{rev} should be refused with a pointer to the named reads, got {err:?}"
        );
        assert!(
            exec.show_file_bytes(&repo, rev, Path::new("merged.txt"))
                .is_err(),
            "the byte-safe sibling refuses stage revs too"
        );
    }
}

#[test]
fn a_path_that_is_not_conflicted_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("clean.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);

    assert!(
        exec()
            .conflict_versions(&repo, Path::new("clean.txt"))
            .is_err(),
        "an unconflicted path must not answer as a conflict with no sides"
    );
}
