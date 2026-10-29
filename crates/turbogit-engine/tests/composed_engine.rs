use std::path::Path;
use std::sync::Arc;

use test_support::git_seed::git;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::GitExecutor;
use turbogit_engine::build_executor;
use turbogit_engine::cli::CliExecutor;

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

fn cli() -> Arc<dyn GitExecutor> {
    Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })
}

#[test]
fn the_engine_serves_reads_and_runs_git_for_the_rest() {
    let tmp = fixture_repo();
    let repo = tmp.path().join("repo");
    let exec = build_executor(&VcsSettings::default());

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
        "the composed adapter must answer the branch listing, whichever side of it reads"
    );

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

    for exec in [cli(), build_executor(&VcsSettings::default())] {
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

#[test]
fn both_adapters_answer_a_range_scoped_log_identically() {
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
    for exec in [cli(), build_executor(&VcsSettings::default())] {
        let opts = turbogit_domain::model::LogOpts {
            branch: Some("mark..HEAD".into()),
            ..Default::default()
        };
        let commits = exec
            .log(&repo, &opts)
            .expect("a range-scoped log answers on every adapter");
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
        "the composed adapter delegates the range rather than refusing it"
    );
}
