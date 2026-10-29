use std::path::PathBuf;
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_domain::model::{Patch, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::{ApplyDirection, GitExecutor};

fn value(text: &str) -> Patch {
    turbogit_engine_api::patch::parse_patch(text)
}

struct Repo {
    path: PathBuf,
    _dir: TempDir,
}

fn temp_repo(name: &str) -> Repo {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path, _dir: dir }
}

fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

const BASE: &str = concat!(
    "alpha\n",
    "bravo\n",
    "charlie\n",
    "delta\n",
    "echo\n",
    "foxtrot\n",
    "golf\n",
    "hotel\n",
    "india\n",
    "juliet\n",
    "kilo\n",
    "lima\n",
    "mike\n",
    "november\n",
    "oscar\n",
    "papa\n",
    "quebec\n",
    "romeo\n",
    "sierra\n",
    "tango\n",
);

const HUNK_ONE_PATCH: &str = concat!(
    "diff --git a/words.txt b/words.txt\n",
    "--- a/words.txt\n",
    "+++ b/words.txt\n",
    "@@ -1,5 +1,5 @@\n",
    " alpha\n",
    "-bravo\n",
    "+BRAVO\n",
    " charlie\n",
    " delta\n",
    " echo\n",
);

#[test]
fn stage_one_hunk_of_two() {
    let repo = temp_repo("stage-one-hunk");
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();

    let ex = executor();
    ex.apply_patch_to_index(&repo.path, &value(HUNK_ONE_PATCH), ApplyDirection::Forward)
        .unwrap();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("-bravo") && staged.contains("+BRAVO"),
        "first hunk should be staged:\n{staged}"
    );
    assert!(
        !staged.contains("QUEBEC"),
        "second hunk must not be staged:\n{staged}"
    );

    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("-quebec") && unstaged.contains("+QUEBEC"),
        "second hunk should remain unstaged:\n{unstaged}"
    );
    assert!(
        !unstaged.contains("BRAVO"),
        "first hunk must stay out of the unstaged diff:\n{unstaged}"
    );

    let status = git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.lines().any(|line| line == "MM words.txt"),
        "file should be partially staged (MM):\n{status}"
    );
}

#[test]
fn reverse_unstage_a_hunk() {
    let repo = temp_repo("reverse-unstage-hunk");
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
    git(&repo.path, &["add", "words.txt"]);

    let ex = executor();
    ex.apply_patch_to_index(&repo.path, &value(HUNK_ONE_PATCH), ApplyDirection::Reverse)
        .unwrap();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("-quebec") && staged.contains("+QUEBEC"),
        "second hunk should remain staged:\n{staged}"
    );
    assert!(
        !staged.contains("BRAVO"),
        "first hunk should have left the index:\n{staged}"
    );

    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("-bravo") && unstaged.contains("+BRAVO"),
        "first hunk should be back in the unstaged diff:\n{unstaged}"
    );
    assert!(
        !unstaged.contains("QUEBEC"),
        "second hunk must stay out of the unstaged diff:\n{unstaged}"
    );

    let status = git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.lines().any(|line| line == "MM words.txt"),
        "file should be partially staged (MM):\n{status}"
    );
}

#[test]
fn intent_to_add_partial_staging_of_untracked_file() {
    let repo = temp_repo("intent-to-add-partial");
    std::fs::write(
        repo.path.join("new.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\n",
    )
    .unwrap();

    let ex = executor();
    ex.add_intent_to_add(&repo.path, &[PathBuf::from("new.txt")])
        .unwrap();
    let creation_patch = concat!(
        "diff --git a/new.txt b/new.txt\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/new.txt\n",
        "@@ -0,0 +1,3 @@\n",
        "+one\n",
        "+two\n",
        "+three\n",
    );
    ex.apply_patch_to_index(&repo.path, &value(creation_patch), ApplyDirection::Forward)
        .unwrap();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("+one") && staged.contains("+two") && staged.contains("+three"),
        "first three lines should be staged:\n{staged}"
    );
    assert!(
        !staged.contains("+four") && !staged.contains("+five") && !staged.contains("+six"),
        "remaining lines must not be staged:\n{staged}"
    );

    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("+four") && unstaged.contains("+five") && unstaged.contains("+six"),
        "remaining lines should stay unstaged:\n{unstaged}"
    );

    let status = git(&repo.path, &["status", "--porcelain"]);
    assert!(
        status.lines().any(|line| line == "AM new.txt"),
        "untracked file should show as partially staged (AM):\n{status}"
    );
}
