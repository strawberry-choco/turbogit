use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use test_support::harness::{painted_text, shell_harness_over_unstyled};
use test_support::{RecordedCall, RecordingExecutor, wait_until};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::git_seed::git;
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, RootId, VcsSettings};
use turbogit_engine::{ApplyDirection, GitExecutor, cli::CliExecutor};

struct Repo {
    path: PathBuf,
}

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

fn seed_two_hunk_change(repo: &Repo) {
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
}

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

fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

#[test]
fn gutter_stage_button_dispatches_forward_patch_application() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gutter-stage");
    seed_two_hunk_change(&repo);

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

    h.get_by_label("words.txt").click();
    h.run();

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
        "gutter unstage control for hunk 1 should appear once the diff loads"
    );

    h.get_by_label("Unstage hunk 1").click();
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

fn row(text: &str, staged: bool) -> String {
    format!("{text} ({})", if staged { "staged" } else { "unstaged" })
}

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

    h.get_by_label("words.txt").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );

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

fn commit_action_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    h.get_by_label("Commit changes")
}

#[test]
fn commit_with_partially_staged_file_commits_index_without_restaging() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "partial-commit");
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

#[test]
fn gutter_stage_on_untracked_file_intents_to_add_then_applies_forward() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked-gutter");
    std::fs::write(
        repo.path.join("new.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\n",
    )
    .unwrap();

    let (state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let mut h = harness(state);

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

fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

fn seed_conflict(repo: &Repo, branch: &str) {
    git(&repo.path, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.path.join("conf.txt"), "side\n").unwrap();
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "side commit"]);
    git(&repo.path, &["checkout", "-q", branch]);
    std::fs::write(repo.path.join("conf.txt"), "main line\n").unwrap();
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(&repo.path, &["merge", "--no-edit", "side"]);
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

    h.get_by_label("C conf.txt").click();
    h.run();

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

#[test]
fn fully_staged_file_leaves_changelist_and_focus_advances_to_next_change() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "focus-advance");
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
        wait_until(15_000, || {
            h.state().ui.preview_change
                == Some(ChangeRef::new(RootId(repo.path.clone().into()), "base.txt"))
        }),
        "preview focus must advance to the next changed file, got {:?}",
        h.state().ui.preview_change
    );
}

#[test]
fn a_partially_staged_file_renders_the_checkbox_middle_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "partial-indicator");
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

    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&row("bravo", true)).is_some()
                && h.query_by_label(&row("BRAVO", false)).is_some()
        }),
        "the staged row stays in the preview beside its pair, and says so while it \
         does: the click moved one line, not the row's own state report"
    );

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
