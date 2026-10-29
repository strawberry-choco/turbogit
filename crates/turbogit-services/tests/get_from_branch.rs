use std::path::{Path, PathBuf};

use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service::{self, BranchFileStatus};

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    pin_eol_lf(&dir);
    git_commit(
        &dir,
        "base.txt",
        "same on both sides\n",
        "the shared commit",
    );
    git_commit(&dir, "both.txt", "one\n", "both.txt exists");
    git(&dir, &["checkout", "-q", "-b", "feature"]);
    git_commit(
        &dir,
        "only-feature.txt",
        "theirs\n",
        "a file only the branch has",
    );
    git_commit(&dir, "both.txt", "two\n", "both.txt rewritten");
    git(&dir, &["checkout", "-q", "main"]);
    git_commit(&dir, "only-main.txt", "mine\n", "a file only main has");
    dir
}

fn rows(dir: &Path, base_is_branch: bool) -> Vec<(String, BranchFileStatus)> {
    integrate_service::branch_file_changes(&engine(), dir, "feature", base_is_branch)
        .expect("the comparison is one git read")
        .into_iter()
        .map(|row| (row.path.to_string_lossy().into_owned(), row.status))
        .collect()
}

#[test]
fn the_three_rungs_read_off_gits_own_section_headers() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    assert_eq!(
        rows(&dir, true),
        vec![
            ("only-feature.txt".into(), BranchFileStatus::Deleted),
            ("only-main.txt".into(), BranchFileStatus::Added),
            ("both.txt".into(), BranchFileStatus::Modified),
        ],
        "grey rows lead, green next, blue last, and `base.txt` — identical on both \
         sides — is not a row"
    );
}

#[test]
fn swapping_the_base_exchanges_the_two_one_sided_rows_and_leaves_the_blue_one() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let before = rows(&dir, true);
    let after = rows(&dir, false);

    assert_eq!(
        after,
        vec![
            ("only-main.txt".into(), BranchFileStatus::Deleted),
            ("only-feature.txt".into(), BranchFileStatus::Added),
            ("both.txt".into(), BranchFileStatus::Modified),
        ],
        "the two one-sided rows traded places"
    );
    assert_eq!(
        before[2], after[2],
        "the differing row is on both sides in either posture, so a swap cannot move it"
    );
    assert_eq!(
        before[0].0, after[1].0,
        "the row that was grey is the one that is now green — the same path, not a \
         re-coloured stranger"
    );
}

#[test]
fn getting_a_file_copies_the_branchs_entire_contents_into_the_working_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    assert_eq!(
        std::fs::read_to_string(dir.join("both.txt")).unwrap(),
        "one\n",
        "the fixture's current side"
    );

    integrate_service::get_file_from_branch(&engine(), &dir, "feature", Path::new("both.txt"))
        .expect("the branch has this path");

    assert_eq!(
        std::fs::read_to_string(dir.join("both.txt")).unwrap(),
        "two\n",
        "the branch's contents, in full"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("base.txt")).unwrap(),
        "same on both sides\n",
        "one row's verb touches one path"
    );
    let status = git(&dir, &["status", "--porcelain"]);
    assert!(
        status.contains("M  both.txt") || status.contains(" M both.txt"),
        "the taken file reads as a local modification, not as a commit: {status}"
    );
}

#[test]
fn a_path_the_branch_does_not_carry_is_refused_not_answered_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let err = integrate_service::get_file_from_branch(
        &engine(),
        &dir,
        "feature",
        Path::new("only-main.txt"),
    )
    .expect_err("the branch has no copy of this path");
    assert!(
        !err.to_string().is_empty(),
        "a refused take says why rather than failing silently"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("only-main.txt")).unwrap(),
        "mine\n",
        "and the file that was there is untouched — no zero-length overwrite"
    );
}
