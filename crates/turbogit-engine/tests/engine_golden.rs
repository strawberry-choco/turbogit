use std::path::{Path, PathBuf};
use std::process::Command;
use test_support::git_seed::{git, pin_eol_lf};
use turbogit_domain::model::{
    ChangeStatus, Commit, DiffOpts, LogOpts, Patch, PatchHeaderLine, PatchLineKind, RootId,
    VcsSettings,
};
use turbogit_engine::GitExecutor;
use turbogit_engine::cli::CliExecutor;

const DATE_1: &str = "@1112911993 +0000";
const DATE_2: &str = "@1234567890 +0000";
const DATE_3: &str = "@1700000000 +0000";
const TIME_1: i64 = 1_112_911_993;
const TIME_2: i64 = 1_234_567_890;
const TIME_3: i64 = 1_700_000_000;

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn run_git_dates(dir: &Path, args: &[&str], author_date: &str, committer_date: &str) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", author_date)
        .env("GIT_COMMITTER_DATE", committer_date)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_git_at(dir: &Path, args: &[&str], date: &str) -> String {
    run_git_dates(dir, args, date, date)
}

fn run_git_failing(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(!out.status.success(), "git {args:?} unexpectedly succeeded");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn configure_identity(repo: &Path) {
    git(repo, &["config", "user.email", "golden@example.com"]);
    git(repo, &["config", "user.name", "Golden Author"]);
    pin_eol_lf(repo);
    git(repo, &["config", "diff.noprefix", "false"]);
}

fn commit_file(repo: &Path, name: &str, content: &str, msg: &str, date: &str) -> String {
    std::fs::write(repo.join(name), content).expect("write fixture file");
    git(repo, &["add", "--", name]);
    run_git_at(repo, &["commit", "-q", "-m", msg], date);
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn temp_repo(name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    configure_identity(&repo);
    commit_file(&repo, "base.txt", "base\n", "base commit", DATE_1);
    (tmp, repo)
}

fn ids(commits: &[Commit]) -> Vec<String> {
    commits.iter().map(|c| c.id.clone()).collect()
}

#[track_caller]
fn assert_patch_one_file(
    patch: &Patch,
    path: &str,
    span: (usize, usize, usize, usize),
    body: &[&str],
) {
    assert_eq!(patch.files.len(), 1, "one section, got:\n{patch}");
    let file = &patch.files[0];
    assert_eq!((&file.old_path[..], &file.new_path[..]), (path, path));
    assert!(
        file.headers.iter().any(|h| matches!(
            h,
            PatchHeaderLine::Index {
                mode: Some(mode),
                ..
            } if mode == "100644"
        )),
        "git's blob-pair line reaches the value:\n{patch}"
    );
    let (want_old, want_new) = (format!("a/{path}"), format!("b/{path}"));
    assert!(
        file.headers
            .iter()
            .any(|h| matches!(h, PatchHeaderLine::Sources { old, new }
                if *old == want_old && *new == want_new)),
        "the source pair reaches the value:\n{patch}"
    );
    assert_eq!(file.hunks.len(), 1, "one hunk, got:\n{patch}");
    let hunk = &file.hunks[0];
    assert_eq!(
        (
            hunk.old_start,
            hunk.old_count,
            hunk.new_start,
            hunk.new_count
        ),
        span,
        "hunk span:\n{patch}"
    );
    let lines: Vec<String> = hunk
        .lines
        .iter()
        .map(|l| {
            let prefix = match l.kind {
                PatchLineKind::Context => ' ',
                PatchLineKind::Added => '+',
                PatchLineKind::Removed => '-',
            };
            format!("{prefix}{}", l.text)
        })
        .collect();
    assert_eq!(lines, body, "patch body mismatch:\n{patch}");
}

#[test]
fn engine_golden_status_clean_repo_is_empty() {
    let (_tmp, repo) = temp_repo("clean");
    let st = engine().status(&repo).expect("status on clean repo");
    assert!(st.changes.is_empty(), "got: {:?}", st.changes);
    assert!(st.conflicted.is_empty());
}

#[test]
fn engine_golden_status_parses_staged_add() {
    let (_tmp, repo) = temp_repo("staged-add");
    std::fs::write(repo.join("fresh.txt"), "fresh\n").expect("write");
    git(&repo, &["add", "fresh.txt"]);

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("fresh.txt"));
    assert_eq!(c.status, ChangeStatus::Added);
    assert!(c.staged);
    assert!(!c.unstaged);
    assert_eq!(c.orig_path, None);
    assert!(c.chunks.is_empty());
    assert!(st.conflicted.is_empty());
}

#[test]
fn engine_golden_status_parses_unstaged_modify() {
    let (_tmp, repo) = temp_repo("unstaged-modify");
    std::fs::write(repo.join("base.txt"), "changed\n").expect("write");

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("base.txt"));
    assert_eq!(c.status, ChangeStatus::Modified);
    assert!(!c.staged);
    assert!(c.unstaged);
    assert_eq!(c.orig_path, None);
}

#[test]
fn engine_golden_status_parses_untracked_file() {
    let (_tmp, repo) = temp_repo("untracked");
    std::fs::write(repo.join("ghost.txt"), "boo\n").expect("write");

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("ghost.txt"));
    assert_eq!(c.status, ChangeStatus::Unversioned);
    assert!(!c.staged);
    assert!(!c.unstaged);
    assert_eq!(c.orig_path, None);
}

#[test]
fn engine_golden_status_parses_deleted_file() {
    let (_tmp, repo) = temp_repo("deleted");
    std::fs::remove_file(repo.join("base.txt")).expect("remove tracked file");

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("base.txt"));
    assert_eq!(c.status, ChangeStatus::Deleted);
    assert!(!c.staged);
    assert!(c.unstaged);
}

#[test]
fn engine_golden_status_parses_renamed_file_with_orig_path() {
    let (_tmp, repo) = temp_repo("renamed");
    git(&repo, &["mv", "base.txt", "renamed.txt"]);

    let st = engine().status(&repo).expect("status");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("renamed.txt"));
    assert_eq!(c.orig_path, Some(PathBuf::from("base.txt")));
    assert_eq!(c.status, ChangeStatus::Renamed);
    assert!(c.staged);
    assert!(!c.unstaged);
}

#[test]
fn engine_golden_status_maps_merge_conflict_to_conflicted() {
    let (_tmp, repo) = temp_repo("conflict");
    git(&repo, &["branch", "feature"]);

    std::fs::write(repo.join("clash.txt"), "main side\n").expect("write main side");
    git(&repo, &["add", "clash.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "main change"], DATE_2);

    git(&repo, &["switch", "feature"]);
    std::fs::write(repo.join("clash.txt"), "feature side\n").expect("write feature side");
    git(&repo, &["add", "clash.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "feature change"], DATE_3);

    git(&repo, &["switch", "main"]);
    let err = run_git_failing(&repo, &["merge", "feature"]);
    assert!(
        err.contains("CONFLICT"),
        "fixture must produce a merge conflict, got: {err}"
    );

    let st = engine().status(&repo).expect("status during conflict");
    assert_eq!(st.changes.len(), 1, "got: {:?}", st.changes);
    let c = &st.changes[0];
    assert_eq!(c.path, PathBuf::from("clash.txt"));
    assert_eq!(c.status, ChangeStatus::Conflicted);
    assert!(!c.staged);
    assert!(!c.unstaged);
    assert_eq!(c.orig_path, None);
    assert_eq!(st.conflicted, vec![PathBuf::from("clash.txt")]);
}

#[test]
fn engine_golden_log_pins_linear_history_fields() {
    let (_tmp, repo) = temp_repo("log-linear");
    let c1 = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let c2 = commit_file(&repo, "two.txt", "two\n", "second subject", DATE_2);
    let c3 = commit_file(
        &repo,
        "three.txt",
        "three\n",
        "subject three\n\nbody alpha\nbody beta",
        DATE_3,
    );

    let commits = engine().log(&repo, &LogOpts::default()).expect("log");
    assert_eq!(commits.len(), 3, "whole history, newest first");
    assert_eq!(ids(&commits), vec![c3.clone(), c2.clone(), c1.clone()]);

    assert!(commits[2].parents.is_empty(), "root commit has no parents");
    assert_eq!(commits[1].parents, vec![c1]);
    assert_eq!(commits[0].parents, vec![c2]);

    for c in &commits {
        assert_eq!(c.author.name, "Golden Author");
        assert_eq!(c.author.email, "golden@example.com");
        assert_eq!(c.committer.name, "Golden Author");
        assert_eq!(c.committer.email, "golden@example.com");
        assert_eq!(c.root, RootId(repo.clone().into()));
    }

    assert_eq!(commits[0].time, TIME_3);
    assert_eq!(commits[0].author.time, TIME_3);
    assert_eq!(commits[0].committer.time, TIME_3);
    assert_eq!(commits[1].time, TIME_2);
    assert_eq!(commits[2].time, TIME_1);

    assert_eq!(
        commits[0].message, "subject three\n\nbody alpha\nbody beta",
        "%B must carry subject AND body"
    );
    assert_eq!(commits[1].message, "second subject");
    assert_eq!(commits[2].message, "base commit");
}

#[test]
fn engine_golden_log_branch_filter_max_count_and_skip() {
    let (_tmp, repo) = temp_repo("log-filter");
    let c1 = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let c2 = commit_file(&repo, "two.txt", "two\n", "second", DATE_2);
    let c3 = commit_file(&repo, "three.txt", "three\n", "third", DATE_3);

    let tail = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(2),
                skip: Some(1),
                ..LogOpts::default()
            },
        )
        .expect("log -n2 --skip=1");
    assert_eq!(ids(&tail), vec![c2.clone(), c1.clone()]);

    git(&repo, &["branch", "topic", &c1]);
    let topic = engine()
        .log(
            &repo,
            &LogOpts {
                branch: Some("topic".to_string()),
                ..LogOpts::default()
            },
        )
        .expect("log topic");
    assert_eq!(ids(&topic), vec![c1.clone()]);

    let past_topic = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(1),
                skip: Some(1),
                branch: Some("topic".to_string()),
                ..LogOpts::default()
            },
        )
        .expect("log -n1 --skip=1 topic");
    assert!(ids(&past_topic).is_empty());

    let one_txt = LogOpts {
        path: Some("two.txt".into()),
        ..LogOpts::default()
    };
    let touching = engine().log(&repo, &one_txt).expect("log -- two.txt");
    assert_eq!(ids(&touching), vec![c2.clone()]);
    let past_path = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(1),
                skip: Some(1),
                ..one_txt
            },
        )
        .expect("log -n1 --skip=1 -- two.txt");
    assert!(ids(&past_path).is_empty());

    let two = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(2),
                ..LogOpts::default()
            },
        )
        .expect("log -n2");
    assert_eq!(ids(&two), vec![c3, c2]);

    let combo = engine()
        .log(
            &repo,
            &LogOpts {
                max_count: Some(1),
                branch: Some("topic".to_string()),
                ..LogOpts::default()
            },
        )
        .expect("log -n1 topic");
    assert_eq!(ids(&combo), vec![c1]);
}

#[test]
fn engine_golden_log_committer_time_mirrors_author_time() {
    let (_tmp, repo) = temp_repo("log-committer-time");
    std::fs::write(repo.join("split.txt"), "split\n").expect("write");
    git(&repo, &["add", "split.txt"]);
    run_git_dates(
        &repo,
        &["commit", "-q", "-m", "split dates"],
        DATE_1,
        DATE_3,
    );

    let commits = engine().log(&repo, &LogOpts::default()).expect("log");
    let top = &commits[0];
    assert_eq!(top.message, "split dates");
    assert_eq!(top.author.time, TIME_1);
    assert_eq!(top.committer.time, TIME_1);
    assert_eq!(top.time, TIME_1);
}

#[test]
fn engine_golden_diff_unstaged_modification_exact_text() {
    let (_tmp, repo) = temp_repo("diff-modify");
    const BASE: &str = "alpha\nbravo\ncharlie\ndelta\necho\n";
    std::fs::write(repo.join("words.txt"), BASE).expect("write");
    git(&repo, &["add", "words.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "words"], DATE_1);

    std::fs::write(repo.join("words.txt"), BASE.replace("bravo", "BRAVO")).expect("rewrite");

    let patch = engine()
        .diff_patch(&repo, &DiffOpts::default())
        .expect("working-tree diff");
    assert_patch_one_file(
        &patch,
        "words.txt",
        (1, 5, 1, 5),
        &[" alpha", "-bravo", "+BRAVO", " charlie", " delta", " echo"],
    );
}

#[test]
fn engine_golden_diff_staged_vs_worktree_semantics() {
    let (_tmp, repo) = temp_repo("diff-staged");
    std::fs::write(repo.join("words.txt"), "one\ntwo\nthree\n").expect("write");
    git(&repo, &["add", "words.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "words"], DATE_1);

    std::fs::write(repo.join("words.txt"), "one\nTWO\nthree\n").expect("stage v2");
    git(&repo, &["add", "words.txt"]);
    std::fs::write(repo.join("words.txt"), "one\nTWO!\nthree\n").expect("worktree v3");

    let wt = engine()
        .diff_patch(&repo, &DiffOpts::default())
        .expect("wt diff");
    assert_patch_one_file(
        &wt,
        "words.txt",
        (1, 3, 1, 3),
        &[" one", "-TWO", "+TWO!", " three"],
    );

    let staged = engine()
        .diff_patch(
            &repo,
            &DiffOpts {
                staged: true,
                ..DiffOpts::default()
            },
        )
        .expect("cached diff");
    assert_patch_one_file(
        &staged,
        "words.txt",
        (1, 3, 1, 3),
        &[" one", "-two", "+TWO", " three"],
    );
}

#[test]
fn engine_golden_diff_ignore_whitespace_flag_mapping() {
    let (_tmp, repo) = temp_repo("diff-ws");
    std::fs::write(repo.join("words.txt"), "alpha\nbravo\ncharlie\n").expect("write");
    git(&repo, &["add", "words.txt"]);
    run_git_at(&repo, &["commit", "-q", "-m", "words"], DATE_1);

    std::fs::write(repo.join("words.txt"), "alpha\nb r a v o\ncharlie\n").expect("whitespace edit");

    let plain = engine()
        .diff_patch(&repo, &DiffOpts::default())
        .expect("plain diff");
    let spelled: Vec<String> = plain
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.lines)
        .map(|l| {
            let prefix = match l.kind {
                PatchLineKind::Context => ' ',
                PatchLineKind::Added => '+',
                PatchLineKind::Removed => '-',
            };
            format!("{prefix}{}", l.text)
        })
        .collect();
    assert!(
        spelled.contains(&"-bravo".to_string()) && spelled.contains(&"+b r a v o".to_string()),
        "whitespace edit must show without the flag:\n{plain}"
    );

    let ignored = engine()
        .diff_patch(
            &repo,
            &DiffOpts {
                ignore_whitespace: true,
                ..DiffOpts::default()
            },
        )
        .expect("ws-insensitive diff");
    assert!(
        ignored.is_empty(),
        "--ignore-all-space must silence a whitespace-only edit:\n{ignored}"
    );
}

#[test]
fn engine_golden_ahead_behind_counts_on_clone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("origin-src");
    std::fs::create_dir_all(&src).expect("src dir");
    git(&src, &["init", "-q", "-b", "main"]);
    configure_identity(&src);
    commit_file(&src, "base.txt", "base\n", "base", DATE_1);

    let clone = tmp.path().join("clone");
    git(
        tmp.path(),
        &["clone", "-q", src.to_str().expect("utf8 src"), "clone"],
    );
    configure_identity(&clone);

    let ex = engine();
    assert_eq!(
        ex.ahead_behind(&clone, "main", "origin/main")
            .expect("ahead/behind in sync"),
        (0, 0)
    );

    let _c2 = commit_file(&clone, "ahead.txt", "ahead\n", "local ahead", DATE_2);
    assert_eq!(
        ex.ahead_behind(&clone, "main", "origin/main")
            .expect("ahead/behind diverged local"),
        (1, 0)
    );

    let _c3 = commit_file(&src, "upstream.txt", "upstream\n", "upstream move", DATE_3);
    git(&clone, &["fetch", "-q", "origin"]);
    assert_eq!(
        ex.ahead_behind(&clone, "main", "origin/main")
            .expect("ahead/behind diverged both"),
        (1, 1)
    );
}

#[test]
fn git2_backend_current_branch_matches_cli_on_unborn_head() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("fresh");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);

    let cli = engine();
    let git2 = turbogit_engine::git2_exec::Git2Executor::new(CliExecutor {
        settings: VcsSettings::default(),
    });

    let expected = cli.current_branch(&repo).expect("CLI on unborn HEAD");
    assert_eq!(expected.as_deref(), Some("main"));
    assert_eq!(
        git2.current_branch(&repo).expect("git2 on unborn HEAD"),
        expected,
        "libgit2 backend must match the CLI on an unborn HEAD"
    );
    assert!(
        git2.is_repo(&repo),
        "a fresh repo must be detected as a repo"
    );
}
