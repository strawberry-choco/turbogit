//! The in-process-reads backend: `GitBackend::InProcessReads` answers the reads
//! `git2` has and runs the git executable for the rest. The factory is the seam
//! (ADR-0001, as corrected by ADR-0022): these tests drive `build_executor` and
//! assert the behavior contract on a real temporary repository. What the two
//! backends *differ* on is pinned next door, in `backend_options.rs`.

use std::path::Path;

use turbogit_domain::model::{GitBackend, VcsSettings};
use turbogit_engine::build_executor;

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

fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    tmp
}

#[test]
fn in_process_reads_backend_serves_reads_and_runs_git_for_the_rest() {
    let tmp = fixture_repo();
    let repo = tmp.path().join("repo");
    let settings = VcsSettings {
        backend: GitBackend::InProcessReads,
        ..VcsSettings::default()
    };
    let exec = build_executor(&settings);

    assert!(
        exec.is_repo(&repo),
        "the composed adapter must serve the read path"
    );
    assert_eq!(
        exec.current_branch(&repo).unwrap().as_deref(),
        Some("main"),
        "the composed adapter must serve reads"
    );
    assert!(
        !exec.branches(&repo).unwrap().is_empty(),
        "the composed adapter must answer libgit2-native reads"
    );

    // `run_raw` is territory `git2` cannot cover, so the composed adapter runs
    // the git executable for it.
    let status = exec
        .run_raw(&repo, &["status".into(), "--porcelain".into()])
        .expect("the composed adapter must run git for what it cannot do in-process");
    assert!(status.is_empty(), "unexpected status output: {status:?}");
}

#[test]
fn the_index_side_of_a_path_answers_its_staged_bytes_not_the_worktree() {
    let tmp = fixture_repo();
    let repo = tmp.path().join("repo");
    let path = Path::new("f.txt");
    let indexed = b"index\0\xff\n";
    std::fs::write(repo.join(path), indexed).unwrap();
    git(&repo, &["add", "f.txt"]);
    std::fs::write(repo.join(path), "worktree only\n").unwrap();

    for backend in [GitBackend::Cli, GitBackend::InProcessReads] {
        let exec = build_executor(&VcsSettings {
            backend,
            ..VcsSettings::default()
        });
        // The named read replaces the `:0` stage rev callers used to pass.
        assert_eq!(exec.index_file_bytes(&repo, path).unwrap(), indexed);
        assert_eq!(exec.show_file_bytes(&repo, "HEAD", path).unwrap(), b"one\n");
        assert!(
            exec.index_file_bytes(&repo, Path::new("missing.txt"))
                .is_err(),
            "an unknown path is an error, not empty bytes — a **Binary change** \
             and an **Image diff** read through here"
        );
    }
}

/// A revision RANGE is a read `git2` has no in-process equivalent for, so the
/// composed adapter runs the git executable for it — the same escape hatch the
/// pickaxe uses a few lines into `Git2Executor::log`. Both backends must answer
/// identically, because the rebase-plan builder asks for `base..HEAD` whatever
/// the settings say, and a refusal there is a dead history editor.
#[test]
fn both_backends_answer_a_range_scoped_log_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    for n in ["base", "c1", "c2"] {
        std::fs::write(repo.join(format!("{n}.txt")), format!("{n}\n")).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", n]);
    }
    git(&repo, &["branch", "mark", "HEAD~2"]);

    let mut answers = Vec::new();
    for backend in [GitBackend::Cli, GitBackend::InProcessReads] {
        let exec = build_executor(&VcsSettings {
            backend,
            ..VcsSettings::default()
        });
        let opts = turbogit_domain::model::LogOpts {
            branch: Some("mark..HEAD".into()),
            ..Default::default()
        };
        let commits = exec
            .log(&repo, &opts)
            .expect("a range-scoped log answers on every backend");
        answers.push(
            commits
                .iter()
                .map(|c| c.message.clone())
                .collect::<Vec<String>>(),
        );
    }
    assert_eq!(
        answers[0],
        vec!["c2".to_string(), "c1".to_string()],
        "the CLI adapter's answer is the one to match"
    );
    assert_eq!(
        answers[1], answers[0],
        "the in-process backend delegates the range rather than refusing it"
    );
}
