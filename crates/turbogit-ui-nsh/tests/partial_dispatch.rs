//! Granular staging from the diff pane, ported from the egui
//! `partial_dispatch` suite: the gutter's hunk verbs reach git forward and
//! reverse, a per-line click stages exactly that line, a conflicted path's
//! gutter refuses, and a commit sends the index as it stands rather than
//! restaging a partially staged file.

use std::path::{Path, PathBuf};

use support::{Project, commit_tab, open_group, seeded_repo, staged_patch};

mod support;

use test_support::git_seed::git;

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

const HUNK_ONE: &str = concat!(
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

fn temp_repo(project: &Project, name: &str) -> PathBuf {
    let repo = seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "work"]);
    repo
}

fn seed_two_hunk_change(repo: &Path) {
    std::fs::write(repo.join("words.txt"), BASE).unwrap();
    git(repo, &["add", "words.txt"]);
    git(repo, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.join("words.txt"), worktree).unwrap();
}

fn stage_first_hunk_by_hand(repo: &Path, parent: &Path) {
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.join("words.txt"), worktree).unwrap();
    let patch_path = parent.join("hunk-one.patch");
    std::fs::write(&patch_path, HUNK_ONE).unwrap();
    git(
        repo,
        &[
            "apply",
            "--cached",
            "--recount",
            patch_path.to_str().unwrap(),
        ],
    );
}

fn open_preview(app: &mut support::Mounted, root_name: &str) {
    app.press(&format!("id:changes:row:{root_name}:words.txt"))
        .unwrap();
    app.wait_for("id:diff:stage-hunk:0")
        .expect("the preview's staging gutter paints");
}

fn two_hunk_app(project: &Project, repo: &Path, name: &str) -> support::Mounted {
    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    open_group(&mut app, name);
    app.wait_for(&format!("id:changes:row:{name}:words.txt"))
        .expect("the changed file row paints");
    open_preview(&mut app, name);
    app
}

#[test]
fn the_gutter_stage_verb_applies_that_hunk_forward() {
    let project = Project::new();
    let repo = temp_repo(&project, "alpha");
    seed_two_hunk_change(&repo);
    let mut app = two_hunk_app(&project, &repo, "alpha");

    app.press("id:diff:stage-hunk:0").unwrap();
    app.wait_until(
        "the first hunk reaches the index",
        std::time::Duration::from_secs(15),
        |_| {
            let staged = staged_patch(&repo);
            staged.contains("+BRAVO") && !staged.contains("QUEBEC")
        },
    )
    .unwrap();
    assert!(
        git(&repo, &["diff"]).contains("+QUEBEC"),
        "the untouched hunk stays unstaged work: {}",
        git(&repo, &["diff"])
    );
}

#[test]
fn the_gutter_unstage_verb_applies_that_hunk_in_reverse() {
    let project = Project::new();
    let repo = temp_repo(&project, "alpha");
    std::fs::write(repo.join("words.txt"), BASE).unwrap();
    git(&repo, &["add", "words.txt"]);
    git(&repo, &["commit", "-q", "-m", "words"]);
    stage_first_hunk_by_hand(&repo, project.path());
    assert!(staged_patch(&repo).contains("+BRAVO"));

    let mut app = two_hunk_app(&project, &repo, "alpha");
    app.press("id:diff:unstage-hunk:0").unwrap();
    app.wait_until(
        "the reverse apply takes the staged hunk out of the index",
        std::time::Duration::from_secs(15),
        |_| staged_patch(&repo).is_empty(),
    )
    .unwrap();
    assert_eq!(
        git(&repo, &["status", "--porcelain"]).trim_end(),
        " M words.txt",
        "the work itself is still there, now wholly unstaged"
    );
}

#[test]
fn per_line_clicks_stage_each_line_and_never_the_other_side() {
    let project = Project::new();
    let repo = temp_repo(&project, "alpha");
    seed_two_hunk_change(&repo);
    let mut app = two_hunk_app(&project, &repo, "alpha");

    // Lower hunk first: staging a line in a later hunk and then one in an
    // earlier hunk leaves each patch's pre-image unshifted. (The reverse
    // order composes a later hunk whose header was written against the
    // pre-stage index; `git apply --cached` recounts it, libgit2's index
    // apply refuses it — an engine finding outside this port.)
    app.press("id:diff:del:17").unwrap();
    app.wait_until(
        "the clicked deletion alone reaches the index",
        std::time::Duration::from_secs(15),
        |_| {
            let staged = staged_patch(&repo);
            staged.contains("-quebec") && !staged.contains("QUEBEC") && !staged.contains("BRAVO")
        },
    )
    .unwrap();
    app.wait_until(
        "the preview re-reads the index before the next click",
        std::time::Duration::from_secs(15),
        |screen| screen.matches("label:quebec (staged)").unwrap_or(false),
    )
    .unwrap();

    app.press("id:diff:del:2").unwrap();
    app.wait_until(
        "the second clicked deletion joins it",
        std::time::Duration::from_secs(15),
        |_| {
            let staged = staged_patch(&repo);
            staged.contains("-bravo") && staged.contains("-quebec")
        },
    )
    .unwrap();

    let staged = staged_patch(&repo);
    assert!(
        !staged.contains("BRAVO") && !staged.contains("QUEBEC"),
        "the unclicked additions stay out of the index:\n{staged}"
    );
    let unstaged = git(&repo, &["diff"]);
    assert!(
        unstaged.contains("+BRAVO") && unstaged.contains("+QUEBEC"),
        "the additions remain unstaged worktree change:\n{unstaged}"
    );
}

#[test]
fn staging_a_hunk_keeps_both_rows_of_that_hunk_in_the_preview() {
    let project = Project::new();
    let repo = temp_repo(&project, "alpha");
    seed_two_hunk_change(&repo);
    let mut app = two_hunk_app(&project, &repo, "alpha");

    app.press("id:diff:stage-hunk:0").unwrap();
    app.wait_until(
        "the stage lands",
        std::time::Duration::from_secs(15),
        |_| !staged_patch(&repo).is_empty(),
    )
    .unwrap();
    app.wait_until(
        "the preview keeps both hunks after the first is staged",
        std::time::Duration::from_secs(10),
        |screen| {
            screen.texts().iter().any(|text| text.contains("QUEBEC"))
                && screen.texts().iter().any(|text| text.contains("BRAVO"))
        },
    )
    .unwrap();
}

#[test]
fn a_conflicted_files_gutter_is_disabled_and_dispatches_nothing() {
    let project = Project::new();
    let repo = temp_repo(&project, "alpha");
    let branch = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("conf.txt"), "side\n").unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", &branch]);
    std::fs::write(repo.join("conf.txt"), "main line\n").unwrap();
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-q", "-m", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();

    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo));
    open_group(&mut app, "alpha");
    app.press("id:changes:row:alpha:conf.txt").unwrap();
    app.wait_for("id:diff:stage-hunk:0")
        .expect("a conflicted file still renders its granular controls");
    let unmerged_before = staged_patch(&repo);
    assert!(
        !app.screen().enabled("id:diff:stage-hunk:0").unwrap(),
        "the gutter stage control is disabled on a conflicted file"
    );
    assert!(
        app.press("id:diff:stage-hunk:0").is_err(),
        "a disabled control cannot be pressed"
    );
    app.wait(std::time::Duration::from_millis(300)).unwrap();
    assert_eq!(
        staged_patch(&repo),
        unmerged_before,
        "no granular patch application may be dispatched for an unmerged path"
    );
    assert_eq!(
        git(&repo, &["diff", "--name-only", "--diff-filter=U"]).trim(),
        "conf.txt",
        "the conflict is still the conflict it was"
    );
}

#[test]
fn a_partially_staged_file_commits_the_index_without_restaging() {
    let project = Project::new();
    let repo = temp_repo(&project, "alpha");
    std::fs::write(repo.join("words.txt"), BASE).unwrap();
    git(&repo, &["add", "words.txt"]);
    git(&repo, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.join("words.txt"), worktree).unwrap();
    stage_first_hunk_by_hand(&repo, project.path());

    let mut app = commit_tab(project.path(), std::slice::from_ref(&repo));
    open_group(&mut app, "alpha");
    app.wait_for("id:changes:row:alpha:words.txt").unwrap();
    app.type_text("id:commit:message", "partial: index as-is")
        .unwrap();
    app.wait_until(
        "the commit enables",
        std::time::Duration::from_secs(15),
        |screen| screen.enabled("id:commit:action").unwrap_or(false),
    )
    .unwrap();
    app.press("id:commit:action").unwrap();
    app.wait_until(
        "the commit lands",
        std::time::Duration::from_secs(20),
        |_| git(&repo, &["log", "-1", "--pretty=%s"]).trim() == "partial: index as-is",
    )
    .unwrap();

    let committed = git(&repo, &["show", "--format=", "--", "words.txt"]);
    assert!(
        committed.contains("+BRAVO") && !committed.contains("QUEBEC"),
        "the commit sends the index as it stood and never re-stages the whole file:\n{committed}"
    );
    assert!(
        git(&repo, &["diff"]).contains("+QUEBEC"),
        "the half left out of the index is still unstaged work"
    );
}
