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
use std::time::{Duration, Instant};
use test_support::harness::painted_text;
use test_support::{RecordedCall, RecordingExecutor};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use turbogit_app::state::AppState;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::{ApplyDirection, GitExecutor, cli::CliExecutor};

// ---------------------------------------------------------------- helpers --

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
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
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: Fn() -> bool>(ms: u64, f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
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

#[test]
fn gutter_unstage_button_dispatches_reverse_patch_application() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gutter-unstage");
    seed_two_hunk_change(&repo);

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
    h.run();

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
}

// ------------------------------------------------------- palette s/u verbs --

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
    seed_two_hunk_change(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
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

/// The primary Commit action button lives on the same row as
/// "Commit and Push..." — disambiguate geometrically (`commit_window.rs`).
fn commit_action_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    let row_y = h.get_by_label("Commit and Push...").rect().center().y;
    let mut on_row: Vec<_> = h
        .get_all_by_label("Commit")
        .filter(|n| (n.rect().center().y - row_y).abs() < 4.0)
        .collect();
    assert_eq!(on_row.len(), 1, "expected exactly one Commit action button");
    on_row.remove(0)
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

    // Include the partially staged file and commit.
    h.get_by_label("M words.txt").click();
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

    // Untracked files live on their own sub-tab; select one for preview.
    h.get_by_label("Unversioned Files").click();
    h.run();
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

#[test]
fn preview_refreshes_after_granular_stage_to_show_remaining_unstaged_changes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "post-op-refresh");
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

    // Stage hunk 1 (the bravo edit); hunk 2 (quebec) stays unstaged.
    h.get_by_label("Stage hunk 1").click();
    h.run();

    // IntelliJ-style refresh (spec R2): the preview must settle on the
    // remaining UNSTAGED changes — quebec still differs, bravo no longer does.
    assert!(
        wait_until(15_000, || {
            let text = painted_text(&h).join("\n");
            text.contains("QUEBEC") && !text.contains("BRAVO")
        }),
        "after staging hunk 1 the preview must show only the remaining \
         unstaged changes"
    );
}

// ------------------------------------------------- fully staged exits list --

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

    // Stage every hunk of words.txt. After the first stage the post-op
    // refresh settles on Local (index↔worktree), where the remaining hunk
    // relabels to "Stage hunk 1".
    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || {
            let text = painted_text(&h).join("\n");
            text.contains("QUEBEC") && !text.contains("BRAVO")
        }),
        "post-op refresh should settle on the remaining unstaged hunk"
    );
    h.get_by_label("Stage hunk 1").click();
    h.run();

    // Story 9: with no unstaged changes left, words.txt leaves the
    // changelist and preview focus advances to the next changed file.
    assert!(
        wait_until(15_000, || {
            let text = painted_text(&h).join("\n");
            !text.contains("M words.txt")
        }),
        "a fully staged file must leave the changelist"
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
fn partially_staged_file_shows_a_coarse_indicator() {
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

    h.get_by_label("M words.txt");

    // Exactly one coarse indicator: on the partially staged file only —
    // never on the plainly modified one.
    assert!(
        wait_until(15_000, || h.get_all_by_label("Partially staged").count()
            == 1),
        "exactly one 'Partially staged' indicator should paint, found {}",
        h.get_all_by_label("Partially staged").count()
    );
}

// ----------------------------------------------------- line-level toggling --

#[test]
fn line_selection_stages_a_sub_hunk_patch_end_to_end() {
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

    // Toggle only the `-bravo` deletion inside hunk 1; the `+BRAVO`
    // addition stays unselected.
    h.get_by_label("bravo").click();
    h.run();

    // Staging the hunk applies the accumulated line selection.
    h.get_by_label("Stage hunk 1").click();
    h.run();

    // End-state verification through git itself (spec layer 3): exactly the
    // selected line reached the index.
    assert!(
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached"]);
            staged.contains("-bravo") && !staged.contains("+BRAVO") && !staged.contains("QUEBEC")
        }),
        "only the selected deletion must be staged, got:\n{}",
        git(&repo.path, &["diff", "--cached"])
    );

    // The dropped addition remains an unstaged local edit: the index
    // already lacks `bravo` (that removal was staged), so relative to the
    // index the worktree only ADDS `BRAVO`.
    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("+BRAVO") && !unstaged.contains("-bravo"),
        "the unselected addition must stay unstaged:\n{unstaged}"
    );
}
