use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_domain::model::{MergeOpts, VcsSettings};
use turbogit_engine::GitExecutor as _;
use turbogit_engine::cli::CliExecutor;

fn commit(dir: &Path, text: &str) {
    let file = dir.join("file.txt");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", text]);
}

fn unsigned_fixture() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "feature-1");
    git(&repo, &["checkout", "-q", "main"]);
    (tmp, repo)
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

#[test]
fn verify_signatures_refuses_unsigned_incoming_commits() {
    let (_tmp, repo) = unsigned_fixture();
    let opts = MergeOpts {
        verify_signatures: true,
        ..Default::default()
    };
    let err = engine()
        .merge(&repo, "feature", &opts)
        .expect_err("unsigned merge must be refused");
    assert!(
        err.to_string().to_lowercase().contains("signature"),
        "expected a signature refusal, got: {err}"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
}

#[test]
fn without_the_option_unsigned_commits_merge_fine() {
    let (_tmp, repo) = unsigned_fixture();
    engine()
        .merge(&repo, "feature", &MergeOpts::default())
        .expect("plain merge of unsigned commits");
}
