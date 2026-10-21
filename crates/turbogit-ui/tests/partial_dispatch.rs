//! Partial staging dispatch lane (spec R2) — headless harness tests.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` over a
//! temporary repository with a [`RecordingExecutor`] injected at the
//! executor boundary (after synchronous registration, per the
//! `push_dialog.rs` pattern), asserting **what the engine was asked to do**
//!
//! Covers the spec's harness layer: gutter stage/unstage actions, palette
//! `s`/`u` verbs, commit-with-partial-selection semantics, conflicted-file
//! blocking.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use test_support::harness::{painted_text, shell_harness_over_unstyled};
use test_support::{RecordedCall, RecordingExecutor, wait_until};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::git_seed::git;
use turbogit_app::state::AppState;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::{ApplyDirection, GitExecutor, cli::CliExecutor};

// ---------------------------------------------------------------- helpers --

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
/// Local repo builder, deliberately NOT a `test_support::git_seed` recipe: it runs
/// `init -q` with no `-b main` (so the default branch is whatever
/// `init.defaultBranch` says, which every recipe pins to `main`) and commits
/// `base.txt` not `README.md`.
fn temp_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path }
}

/// 20-line file whose edits at line 2 (`bravo`) and line 17 (`quebec`) sit
/// far enough apart that git reports two independent hunks.
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

/// Seed `words.txt` committed at BASE, then diverge the worktree with two
/// single-line edits → a two-hunk working-tree diff.
fn seed_two_hunk_change(repo: &Repo) {
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
}

/// [`app_state`] with an injected recording engine — the swap happens AFTER
/// synchronous registration, so setup reads never reach the recorder.
fn app_state_with_recorder(
    project_dir: &Path,
    roots: &[PathBuf],
) -> (AppState, Arc<RecordingExecutor>) {
    let recorder = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let state = AppState::for_roots(project_dir, roots)
        .with_executor(recorder.clone() as Arc<dyn GitExecutor>);
    (state, recorder)
}

/// Headless harness driving the full app UI with event draining per frame.
///
/// Kept as [`shell_harness_over_unstyled`]: the changelist column's geometry is read
/// off the frame, and the dark tokens plus the embedded JetBrains Mono stack would
/// re-measure it. Measured — [`shell_harness_over`] turns **9 of 12 tests red**.
fn harness(state: AppState) -> Harness<'static, AppState> {
    // Sized like the commit_window suite: the changelist column needs the
    // full two-pane width once the workspace sidebar (issue #05) claims
    // the left edge. The first frames also relayout (embedded fonts take
    // effect at pass 2), so settle before any clicks (test-support
    // `settle`'s rationale).
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

// ------------------------------------------------------------------ tests --

#[test]
fn gutter_stage_button_dispatches_forward_patch_application() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gutter-stage");
    seed_two_hunk_change(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    // Select the file so its diff preview renders.
    h.get_by_label("words.txt").click();
    h.run();

    // Wait for the async diff load to land and the gutter controls to paint.
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "gutter stage control for hunk 1 should appear once the diff loads"
    );

    h.get_by_label("Stage hunk 1").click();
    h.run();

    assert!(
        wait_until(15_000, || recorder.recorded().contains(
            &RecordedCall::ApplyPatch {
                direction: ApplyDirection::Forward
            }
        )),
        "clicking the gutter + must ask the engine to apply the composed \
         patch forward, recorded={:?}",
        recorder.recorded()
    );
}

/// Stage hunk 1 of `words.txt` (the `bravo` edit) with git itself, leaving hunk 2
/// (`quebec`) as an unstaged worktree change.
///
/// The gutter `−` has nothing to reverse-apply unless something is staged, and
/// the combined view makes that visible: the pane shows the staged row beside the
/// unstaged one. So an unstage test needs a staged half, and this puts one there
/// without going through the app's own stage path first.
fn stage_first_hunk_by_hand(repo: &Repo, parent: &Path) {
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
    let patch_path = parent.join("hunk-one.patch");
    std::fs::write(
        &patch_path,
        concat!(
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
        ),
    )
    .unwrap();
    git(
        &repo.path,
        &[
            "apply",
            "--cached",
            "--recount",
            patch_path.to_str().unwrap(),
        ],
    );
}

#[test]
fn gutter_unstage_button_dispatches_reverse_patch_application() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gutter-unstage");
    // Committed at BASE, then hunk 1 staged by hand and hunk 2 left unstaged.
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    stage_first_hunk_by_hand(&repo, parent.path());

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    // Select the file so its diff preview renders.
    h.get_by_label("words.txt").click();
    h.run();

    assert!(
        wait_until(15_000, || h.query_by_label("Unstage hunk 1").is_some()),
        "gutter unstage control for hunk 1 should appear once the diff loads"
    );

    h.get_by_label("Unstage hunk 1").click();
    // The op's completion lands over several frames: dispatch, the worker, the
    // `OpCompleted` → refresh → settle route, and the harness only pumps them
    // when asked.
    for _ in 0..8 {
        h.run();
    }

    assert!(
        wait_until(15_000, || recorder.recorded().contains(
            &RecordedCall::ApplyPatch {
                direction: ApplyDirection::Reverse
            }
        )),
        "clicking the gutter − must ask the engine to reverse-apply the \
         composed patch against the index, recorded={:?}",
        recorder.recorded()
    );
    // Graded on git's answer, not on the dispatch: before the preview was pinned
    // this same click reached git as a reverse apply of an `index ↔ worktree`
    // hunk, was refused with `error: patch failed`, and left the index alone.
    // Here the staged hunk is the only staged content, so success is an index
    // that matches HEAD again with the worktree still diverging.
    assert!(
        wait_until(15_000, || git(&repo.path, &["diff", "--cached"]).is_empty()
            && git(&repo.path, &["status", "--porcelain"]).trim_end()
                == " M words.txt"),
        "the reverse apply must actually take the staged hunk out of the index, \
         staged={:?} status={:?} toast={:?} recorded={:?}",
        git(&repo.path, &["diff", "--cached"]),
        git(&repo.path, &["status", "--porcelain"]),
        h.state().ui.toast.clone(),
        recorder.recorded()
    );
    assert!(
        h.state()
            .ui
            .toast
            .as_ref()
            .is_none_or(|t| !matches!(t.kind, turbogit_app::state::ToastKind::Error)),
        "git accepted it, so no error toast"
    );
}

// ------------------------------------------------------- palette s/u verbs --

/// A changed diff row's accessibility label, spelled the way the pane spells it:
/// the row's text, then the state the per-line oracle answers for it. The state is
/// what a click is *about*, and it is the only thing in the label that a click
/// changes, so this is both the handle a test addresses a row by and the assertion
/// that the row's own report moved.
fn row(text: &str, staged: bool) -> String {
    format!("{text} ({})", if staged { "staged" } else { "unstaged" })
}

/// Open the palette filtered to `query` and click `label` (the
/// `feedback_chrome.rs` pattern: reachable through search + click).
fn run_palette_entry(h: &mut Harness<'_, AppState>, query: &str, label: &str) {
    {
        let st = h.state_mut();
        st.ui.command_palette = true;
        st.ui.command_query = query.to_owned();
    }
    h.run();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

#[test]
fn palette_stage_verb_dispatches_forward_apply_for_current_hunk() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "palette-stage");
    seed_two_hunk_change(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    // Select the file so its diff preview renders.
    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );

    // Aim the current hunk at hunk 1 (0-based index 0).
    h.state_mut().ui.diff_current_hunk = 0;
    h.run();

    run_palette_entry(&mut h, "stage hunk", "Stage Hunk");

    assert!(
        wait_until(15_000, || recorder.recorded().contains(
            &RecordedCall::ApplyPatch {
                direction: ApplyDirection::Forward
            }
        )),
        "palette `s` must apply the current hunk's composed patch forward, \
         recorded={:?}",
        recorder.recorded()
    );
}

#[test]
fn palette_stage_verb_no_ops_without_a_previewed_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "palette-noop");
    seed_two_hunk_change(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    // No file selected for preview: the verb must be a silent no-op.
    assert!(h.state().ui.preview_change.is_none());
    run_palette_entry(&mut h, "stage hunk", "Stage Hunk");

    std::thread::sleep(Duration::from_millis(300));
    h.run();
    assert!(
        !recorder
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::ApplyPatch { .. })),
        "palette `s` without a previewed file must not reach the engine, \
         recorded={:?}",
        recorder.recorded()
    );
}

#[test]
fn palette_unstage_verb_dispatches_reverse_apply_for_current_hunk() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "palette-unstage");
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    stage_first_hunk_by_hand(&repo, parent.path());

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Unstage hunk 1").is_some()),
        "diff preview should load"
    );

    // Aim the current hunk at hunk 1 (0-based index 0).
    h.state_mut().ui.diff_current_hunk = 0;
    h.run();

    run_palette_entry(&mut h, "unstage hunk", "Unstage Hunk");

    assert!(
        wait_until(15_000, || recorder.recorded().contains(
            &RecordedCall::ApplyPatch {
                direction: ApplyDirection::Reverse
            }
        )),
        "palette `u` must reverse-apply the current hunk's patch against the \
         index, recorded={:?}",
        recorder.recorded()
    );
}

// ------------------------------------------ commit-with-partial-selection --

/// The primary Commit action's main part (issue 07: the `Commit ▾` split
/// button, labeled "Commit changes" via `WidgetInfo`).
fn commit_action_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    h.get_by_label("Commit changes")
}

#[test]
fn commit_with_partially_staged_file_commits_index_without_restaging() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "partial-commit");
    // Committed at BASE, worktree diverged by two edits, first hunk already
    // staged by hand → the file sits partially staged (MM) before the app opens.
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
    let hunk_one = concat!(
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
    let patch_path = parent.path().join("hunk-one.patch");
    std::fs::write(&patch_path, hunk_one).unwrap();
    git(
        &repo.path,
        &[
            "apply",
            "--cached",
            "--recount",
            patch_path.to_str().unwrap(),
        ],
    );

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);
    for _ in 0..3 {
        h.run();
    }

    // Include the partially staged file and commit.
    h.get_by_label("words.txt").click();
    h.state_mut().ui.commit_message = "partial: index as-is".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || recorder
            .recorded()
            .contains(&RecordedCall::CommitIndex)),
        "the commit must go through commit_index, recorded={:?}",
        recorder.recorded()
    );
    assert!(
        !recorder.recorded().iter().any(|c| matches!(
            c,
            RecordedCall::Add(paths) if paths.iter().any(|p| p.ends_with("words.txt"))
        )),
        "a partially staged file must NOT be re-staged whole — that would \
         blow away the granular selection, recorded={:?}",
        recorder.recorded()
    );
}

// --------------------------------------------------- untracked partial stage --

#[test]
fn gutter_stage_on_untracked_file_intents_to_add_then_applies_forward() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked-gutter");
    // An untracked file whose staged subset is narrower than the whole file.
    std::fs::write(
        repo.path.join("new.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\n",
    )
    .unwrap();

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    // Untracked files are ordinary rows of the one flat list — select one for
    // preview.
    h.get_by_label("new.txt").click();
    h.run();

    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "an untracked file's preview must offer granular staging controls"
    );

    h.get_by_label("Stage hunk 1").click();
    h.run();

    assert!(
        wait_until(15_000, || {
            let recorded = recorder.recorded();
            let intent = recorded.iter().position(|c| matches!(
                c,
                RecordedCall::AddIntentToAdd(paths) if paths.iter().any(|p| p.ends_with("new.txt"))
            ));
            let patch = recorded.iter().position(|c| {
                matches!(
                    c,
                    RecordedCall::ApplyPatch {
                        direction: ApplyDirection::Forward
                    }
                )
            });
            match (intent, patch) {
                (Some(i), Some(p)) => i < p,
                _ => false,
            }
        }),
        "staging part of an untracked file must mark it intent-to-add BEFORE \
         applying the composed patch, recorded={:?}",
        recorder.recorded()
    );
}

// ---------------------------------------------------- conflicted blocking --

/// Run `git` without asserting success (a merge that conflicts).
/// Run `git` WITHOUT asserting success, and deliberately NOT
/// `test_support::git_seed::git`, which asserts it: `seed_conflict` ends in an
/// expected merge failure, and the unresolved state is the precondition.
fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

/// Create a real merge conflict in `conf.txt` on `repo`'s default branch.
fn seed_conflict(repo: &Repo, branch: &str) {
    git(&repo.path, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.path.join("conf.txt"), "side\n").unwrap();
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "side commit"]);
    git(&repo.path, &["checkout", "-q", branch]);
    std::fs::write(repo.path.join("conf.txt"), "main line\n").unwrap();
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(&repo.path, &["merge", "--no-edit", "side"]); // conflicts
}

#[test]
fn conflicted_file_gutter_controls_are_disabled_and_never_dispatch() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "conflict-block");
    let branch = git(&repo.path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    seed_conflict(&repo, &branch);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    // Select the conflicted file from the Merge conflicts group.
    h.get_by_label("C conf.txt").click();
    h.run();

    // Granular controls render (disabled) rather than disappearing…
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "conflicted files must still render their granular controls"
    );
    assert!(
        h.get_by_label("Stage hunk 1")
            .accesskit_node()
            .is_disabled(),
        "gutter stage control must be disabled on a conflicted file"
    );

    // …and activating one must never reach the engine.
    h.get_by_label("Stage hunk 1").click();
    h.run();
    std::thread::sleep(Duration::from_millis(300));
    h.run();
    assert!(
        !recorder
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::ApplyPatch { .. })),
        "conflicted files resolve through the conflict modal — no granular \
         patch application may be dispatched, recorded={:?}",
        recorder.recorded()
    );
}

// ---------------------------------------------------- post-op diff refresh --

/// Story 9's settlement is a file-list question now: a row click cannot empty the
/// preview, because the preview reads `HEAD ↔ worktree` and staging does not change
/// it. What "this file is finished" means is git's status for the file, which is
/// what the flat list's buckets are keyed on. Fully staging `words.txt` therefore
/// records its completion and advances preview focus, while the pane beside it
/// keeps showing both hunks until the second one is clicked.
#[test]
fn fully_staged_file_leaves_changelist_and_focus_advances_to_next_change() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "focus-advance");
    // Two changed files: words.txt (two hunks) and base.txt (one change).
    seed_two_hunk_change(&repo);
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );

    // Stage every hunk of words.txt. After the first click the combined view
    // neither relabels nor renumbers: hunk 2 is still "Stage hunk 2", because
    // staging a line cannot change `git diff HEAD`.
    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || {
            let text = painted_text(&h).join("\n");
            text.contains("QUEBEC") && text.contains("BRAVO")
        }),
        "the preview keeps showing both hunks after the first is staged"
    );
    h.get_by_label("Stage hunk 2").click();
    h.run();

    // Story 9: with no unstaged changes left, words.txt is fully staged. The
    // completion is recorded (it drives focus advancement — the flat list
    // keeps the fully staged file visible with a ticked box), and preview
    // focus advances to the next changed file.
    assert!(
        wait_until(15_000, || !h.state().ui.granularly_completed.is_empty()),
        "the granular completion must be recorded"
    );
    assert!(
        h.query_by_label("words.txt").is_some(),
        "the flat list keeps a fully staged file visible"
    );
    assert!(
        h.query_by_label("Unstage words.txt (staged)").is_some(),
        "and its checkbox now reads fully staged"
    );
    assert!(
        wait_until(15_000, || h.state().ui.preview_change
            == Some(PathBuf::from("base.txt"))),
        "preview focus must advance to the next changed file, got {:?}",
        h.state().ui.preview_change
    );
}

// ------------------------------------------------- partial-stage indicator --

#[test]
fn a_partially_staged_file_renders_the_checkbox_middle_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "partial-indicator");
    // words.txt partially staged by hand (MM); base.txt plainly modified.
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
    let patch_path = parent.path().join("hunk-one.patch");
    std::fs::write(
        &patch_path,
        concat!(
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
        ),
    )
    .unwrap();
    git(
        &repo.path,
        &[
            "apply",
            "--cached",
            "--recount",
            patch_path.to_str().unwrap(),
        ],
    );
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let h = harness(state);

    h.get_by_label("words.txt");

    // The partially staged file's box is in its middle state and says so; the
    // plainly modified one beside it is empty. The coarse warning dot is gone:
    // the checkbox is where partial staging is said now.
    assert!(
        wait_until(15_000, || h
            .query_by_label("Stage all of words.txt (partially staged)")
            .is_some()),
        "the partially staged file's checkbox must read its middle state"
    );
    h.get_by_label("Stage base.txt (unstaged)");
    assert!(
        !painted_text(&h)
            .iter()
            .any(|t| t.contains("Partially staged")),
        "the old warning dot is gone — the checkbox carries the state"
    );
}

/// **Partial staging is reachable from the diff viewer and lands in the index**
/// — the path that makes the middle checkbox state reachable at all. Read back
/// out of git, not out of the window's own state.
#[test]
fn staging_one_hunk_from_the_diff_viewer_makes_the_row_partially_staged() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "partial-from-viewer");
    seed_two_hunk_change(&repo);

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );

    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || git(&repo.path, &["diff", "--cached"])
            .contains("+BRAVO")),
        "one hunk reached the index"
    );

    assert!(
        wait_until(15_000, || h
            .query_by_label("Stage all of words.txt (partially staged)")
            .is_some()),
        "the row's checkbox must show the middle state after a partial stage"
    );
}

// ----------------------------------------------------- line-level staging --

#[test]
fn per_line_clicks_stage_each_line_end_to_end() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "line-toggle");
    seed_two_hunk_change(&repo);

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );

    // Click the `-bravo` deletion row: the click IS the stage, one git
    // operation, no second control. The row is addressed by the label the pane
    // gives a changed line: its text and the state the per-line oracle answers.
    h.get_by_label(&row("bravo", false)).click();
    h.run();
    assert!(
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached"]);
            staged.contains("-bravo") && !staged.contains("+BRAVO") && !staged.contains("QUEBEC")
        }),
        "the clicked deletion alone must reach the index, got:\n{}",
        git(&repo.path, &["diff", "--cached"])
    );

    // The clicked row is still on screen, and so is its pair. A pinned
    // `index ↔ worktree` preview would have lost the staged row to the next
    // click, because a staged line is absent from that patch; the assertion here
    // is that the combined view is byte-stable under the action the pane offers,
    // which is what makes an immediate click survivable at all. The round trip
    // back out of the index is graded in `combined_preview_toggle.rs`.
    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&row("bravo", true)).is_some()
                && h.query_by_label(&row("BRAVO", false)).is_some()
        }),
        "the staged row stays in the preview beside its pair, and says so while it \
         does: the click moved one line, not the row's own state report"
    );

    // Click the `-quebec` deletion row of the other hunk: one operation per
    // line, and both deletions are now staged while both additions are not.
    h.get_by_label(&row("quebec", false)).click();
    h.run();
    assert!(
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached"]);
            staged.contains("-bravo") && staged.contains("-quebec")
        }),
        "both clicked deletions must be staged, got:\n{}",
        git(&repo.path, &["diff", "--cached"])
    );

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        !staged.contains("BRAVO") && !staged.contains("QUEBEC"),
        "the unclicked additions must stay out of the index:\n{staged}"
    );
    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("+BRAVO") && unstaged.contains("+QUEBEC"),
        "the additions remain unstaged worktree change:\n{unstaged}"
    );
}

/// An unstage of a hunk with nothing staged is no operation at all.
///
/// The defect this surface can produce, and the one the shape of the composed
/// patch decides: a gutter `−` whose reverse apply is cut from the
/// `index ↔ worktree` patch reaches git with a new side the index never held,
/// because that patch's post-image is the worktree and a reverse apply matches
/// new sides against the index. git refuses it with
/// `error: patch failed: words.txt:1`, the pane surfaces an error toast, and the
/// repository is untouched.
///
/// The preview reads `HEAD ↔ worktree` and the unstage composes from the retained
/// `HEAD ↔ index` section, so the click is only dispatched when there is a staged
/// line to take out. Nothing is staged on this fixture, so the honest answer is no
/// operation at all: no engine call,
/// no toast, no settlement armed. The half where git is actually asked and accepts
/// is `gutter_unstage_button_dispatches_reverse_patch_application`, which stages a
/// hunk first and grades the index afterwards.
#[test]
fn gutter_unstage_on_a_fully_unstaged_hunk_does_nothing_and_leaves_the_repo_untouched() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "unstage-noop");
    seed_two_hunk_change(&repo);
    let worktree_before = std::fs::read_to_string(repo.path.join("words.txt")).unwrap();

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );

    h.get_by_label("Unstage hunk 1").click();
    // The op's completion lands over several frames: dispatch, the worker, the
    // `OpCompleted` → refresh → settle route, and the harness only pumps them
    // when asked.
    for _ in 0..8 {
        h.run();
    }
    std::thread::sleep(Duration::from_millis(300));
    h.run();

    assert!(
        !recorder
            .recorded()
            .iter()
            .any(|c| matches!(c, RecordedCall::ApplyPatch { .. })),
        "a hunk with no staged row has nothing to reverse-apply, recorded={:?}",
        recorder.recorded()
    );
    assert!(
        !matches!(
            h.state().ui.toast.as_ref().map(|t| t.kind),
            Some(turbogit_app::state::ToastKind::Error)
        ),
        "the refused apply that used to surface as an error toast is gone: {:?}",
        h.state().ui.toast
    );
    assert!(
        h.state().ui.pending_granular.is_none(),
        "a no-op must not arm completion settlement"
    );

    // And git itself untouched: nothing was staged before, nothing is staged
    // after, and the worktree never moved.
    assert!(
        git(&repo.path, &["diff", "--cached"]).is_empty(),
        "index moved"
    );
    assert_eq!(
        git(&repo.path, &["status", "--porcelain"]).trim_end(),
        " M words.txt",
        "status moved"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path.join("words.txt")).unwrap(),
        worktree_before,
        "worktree moved"
    );
}

/// The visible half of the pin, and the reason the pending set could be deleted:
/// a row click leaves the combined view showing the whole file. Both rows of the
/// edited line stay on screen after one of them is staged, so no row shifts,
/// disappears or renumbers under the only action the pane offers.
#[test]
fn the_pinned_preview_keeps_the_staged_and_the_unstaged_row_both_visible() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "preview-stability");
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    stage_first_hunk_by_hand(&repo, parent.path());

    let (state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || {
            let text = painted_text(&h).join("\n");
            text.contains("BRAVO") && text.contains("QUEBEC")
        }),
        "the combined view shows the staged half and the unstaged half at once"
    );

    // The pane header labels come from the target's own revisions, so the pinned
    // read prints `HEAD` and `Working tree`; that half is the snapshot surface
    // (`pane_labels` in `view.rs` is the code, and the side-by-side band it paints
    // is not in this harness's default view mode).

    // Toggle the STAGED row back out. The frames are pumped inside the poll,
    // because the post-op refresh answers `Waiting` for a frame or two before the
    // rows are painted again. Staging the *other* hunk instead would finish the
    // file and let story 9's settlement move the preview, which is
    // `fully_staged_file_leaves_changelist_and_focus_advances_to_next_change`.
    //
    // The label addresses it *because* it is staged: this row was put into the
    // index by hand before the app started, so the pane calls it `staged` and no
    // other row on screen carries that word.
    let before = painted_text(&h).join("\n");
    h.get_by_label(&row("BRAVO", true)).click();
    assert!(
        wait_until(15_000, || {
            h.run();
            let text = painted_text(&h).join("\n");
            text.contains("BRAVO")
                && text.contains("bravo")
                && text.contains("QUEBEC")
                && git(&repo.path, &["diff", "--cached"]).is_empty()
        }),
        "unstaging the line the reader clicked moved no row and hid no row; \
         before={before}"
    );
}
