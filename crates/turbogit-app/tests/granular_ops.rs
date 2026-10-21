//! Granular op interface tests (spec R2 stories 3/8/9) — direct, headless.
//!
//! Exercises [`turbogit_app::granular::dispatch`] and — through its
//! production trigger, the `OpCompleted` → refresh → settle path in
//! [`turbogit_app::state::AppState::drain_events`] — the completion settlement
//! over a real temporary repository ([`AppState::for_roots`], whose dispatch
//! seam runs the work inline). No UI rendering: callers pass pure intent exactly as the diff
//! viewer's gutter controls and the palette verbs do, and assertions observe
//! **what reached the engine** (via [`test_support::RecordingExecutor`]) and the
//! resulting repository state via real `git` commands.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::{RecordedCall, RecordingExecutor};

use turbogit_app::diff_model::RowKind;
use turbogit_app::granular::{self, Direction, HunkTarget};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::{ApplyDirection, GitExecutor};

// ---------------------------------------------------------------- helpers --

/// Short `git status --porcelain` XY code for one path.
fn porcelain_code(repo: &Path, rel: &str) -> String {
    let out = git(repo, &["status", "--porcelain"]);
    out.lines()
        .find_map(|l| {
            l[3..]
                .trim()
                .eq_ignore_ascii_case(rel)
                .then(|| l[..2].to_owned())
        })
        .unwrap_or_else(|| panic!("{rel} not in status:\n{out}"))
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
///
/// `git_seed::repo_with_one_commit` is these same steps, so that recipe owns
/// them; only the seeded file differs and no assertion here reads it.
fn temp_repo(parent: &Path, name: &str) -> Repo {
    Repo {
        path: repo_with_one_commit(parent, name),
    }
}

/// 20-line base content; edits at line 2 (`bravo`) and line 17 (`quebec`)
/// sit far enough apart that git reports two independent hunks.
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

/// Commit `words.txt` at BASE, then diverge the worktree with two single-line
/// edits → a two-hunk working-tree diff.
fn seed_two_hunk_change(repo: &Repo) {
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();
}

/// [`AppState`] over the repo with an injected recording engine — the swap
/// happens AFTER synchronous registration, so setup reads never reach the
/// recorder (the `partial_dispatch.rs` pattern).
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

/// Warm the two doors a granular op reads: the root's retained sections, which
/// is what an operation composes from, and the pinned preview read, which is what
/// an untracked path composes from.
///
/// This used to spell the cache key out by hand so it could stuff a string into
/// the field — which pinned the very coupling this branch exists to remove: a
/// test that agrees with the module's key format by copying it proves nothing
/// when the two drift. Driving `read` asks the module instead, and the patch
/// text it answers with is the one `git diff HEAD` really produces for the
/// fixture.
fn seed_preview_cache(state: &mut AppState, root: &Path, rel: &str) {
    state.ui.diff_policies = turbogit_domain::model::DiffPolicies::default();
    let root_id = RootId(root.to_path_buf().into());
    state.ensure_hunk_stats(&root_id);
    let target = DiffTarget::working_tree(
        root.to_path_buf(),
        turbogit_domain::model::DiffPolicies::default(),
        Some(PathBuf::from(rel)),
    );
    assert!(
        matches!(state.read(target.clone()), Read::Waiting),
        "the preview slot starts cold"
    );
    state.drain_events();
    assert!(
        matches!(state.read(target), Read::Fresh(_) | Read::Empty),
        "the preview read settles for the fixture's own diff"
    );
}

// ------------------------------------------------------------------ tests --

#[test]
fn granular_dispatch_stages_whole_hunk_forward() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "whole-hunk");
    seed_two_hunk_change(&repo);

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "words.txt");

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );

    assert_eq!(
        porcelain_code(&repo.path, "words.txt"),
        "MM",
        "staging hunk 0 must leave hunk 1 unstaged (partially staged MM)"
    );
    state.drain_events();

    assert!(
        recorder.recorded().contains(&RecordedCall::ApplyPatch {
            direction: ApplyDirection::Forward
        }),
        "whole-hunk stage must forward-apply the composed patch, recorded={:?}",
        recorder.recorded()
    );
    let staged = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
    assert!(staged.contains("+BRAVO"), "hunk 0 staged, got:\n{staged}");
    assert!(
        !staged.contains("QUEBEC"),
        "hunk 1 must stay out of the index, got:\n{staged}"
    );
}

#[test]
fn granular_dispatch_stages_a_single_line_target() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "line-stage");
    seed_two_hunk_change(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "words.txt");

    // One row click's target: the `+BRAVO` addition, addressed by the worktree
    // gutter the combined view numbers it with. Nothing names a direction,
    // because the row's staged-ness is the direction.
    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Row(RowKind::Add, 0, 2),
    );

    assert!(
        git(&repo.path, &["diff", "--cached", "--", "words.txt"]).contains("+BRAVO"),
        "the selected line must land in the index"
    );
    state.drain_events();

    let staged = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
    assert!(staged.contains("+BRAVO"), "selected line staged:\n{staged}");
    assert!(
        !staged.contains("QUEBEC"),
        "the other hunk must stay out, got:\n{staged}"
    );
    let unstaged = git(&repo.path, &["diff", "--", "words.txt"]);
    assert!(
        unstaged.contains("+QUEBEC"),
        "hunk 1 remains unstaged worktree change:\n{unstaged}"
    );
    assert_eq!(
        porcelain_code(&repo.path, "words.txt"),
        "MM",
        "a line-staged file is partially staged"
    );
}

#[test]
fn granular_dispatch_stages_exactly_the_selected_character_range() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "char-range");
    // One-hunk edit of a single long line; the drag happened inside the
    // addition, selecting `calculated_value` (chars 8..24 of its body).
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let x = 1;\n}\n",
    )
    .unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let calculated_value = compute(x);\n}\n",
    )
    .unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "code.rs");

    granular::dispatch(
        &mut state,
        PathBuf::from("code.rs"),
        HunkTarget::Chars(2, 8, 24),
    );
    state.drain_events();

    assert!(
        recorder.recorded().contains(&RecordedCall::ApplyPatch {
            direction: ApplyDirection::Forward
        }),
        "char-range stage must forward-apply the composed patch, recorded={:?}",
        recorder.recorded()
    );
    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("+calculated_value\n"),
        "the index must gain exactly the selected bytes:\n{staged}"
    );
    assert!(
        !staged.contains("compute"),
        "the unselected remainder of the line must stay out of the index:\n{staged}"
    );
    let unstaged = git(&repo.path, &["diff"]);
    assert!(
        unstaged.contains("-    let x = 1;") && unstaged.contains("+    let calculated_value"),
        "the full line edit remains an unstaged worktree change:\n{unstaged}"
    );
}

#[test]
fn char_drag_release_stages_the_range_and_a_collapsed_drag_stages_nothing() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "drag-release");
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let x = 1;\n}\n",
    )
    .unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let calculated_value = compute(x);\n}\n",
    )
    .unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "code.rs");
    let path = Some(PathBuf::from("code.rs"));

    // The gesture the viewer's drag forwards: anchor, move, release. The draft
    // carries the row's worktree gutter, which is what the release addresses.
    granular::begin_char_drag(&mut state, &path, 0, 1, 2, 8);
    granular::extend_char_drag(&mut state, &path, 0, 1, 24);
    assert!(
        granular::finish_char_drag(&mut state, &path, 0, 1),
        "a moved range stages at release"
    );
    assert!(
        state.ui.char_drag.is_none(),
        "the draft never outlives the gesture"
    );
    state.drain_events();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("+calculated_value\n") && !staged.contains("compute"),
        "the release must stage exactly the dragged bytes:\n{staged}"
    );
    assert!(
        recorder.recorded().contains(&RecordedCall::ApplyPatch {
            direction: ApplyDirection::Forward
        }),
        "the release routes through the same forward-apply seam, recorded={:?}",
        recorder.recorded()
    );

    // A drag that never moved is a click, and the click route owns the line:
    // the release itself must stage nothing.
    granular::begin_char_drag(&mut state, &path, 0, 0, 0, 4);
    let staged_before = git(&repo.path, &["diff", "--cached"]);
    assert!(
        !granular::finish_char_drag(&mut state, &path, 0, 0),
        "a collapsed range must report that it staged nothing"
    );
    assert!(state.ui.char_drag.is_none(), "the draft dies either way");
    state.drain_events();
    assert_eq!(
        git(&repo.path, &["diff", "--cached"]),
        staged_before,
        "a collapsed drag must not reach the index"
    );
}

#[test]
fn granular_dispatch_routes_untracked_stage_through_intent_to_add() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked");
    // Created BEFORE registration so the snapshot lists it as Unversioned.
    std::fs::write(repo.path.join("new.txt"), "one\ntwo\nthree\n").unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    // An Unversioned path never reaches `git diff`: the read synthesizes the
    // creation diff itself, in the exact shape partial-staging proves
    // appliable (pinned by `diff_load`'s own tests), and spends no engine call
    // doing it — which is what keeps the two-call assertion below honest.
    seed_preview_cache(&mut state, &repo.path, "new.txt");

    granular::dispatch(
        &mut state,
        PathBuf::from("new.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );

    assert_eq!(
        recorder.recorded().len(),
        2,
        "untracked staging routes two engine calls, recorded={:?}",
        recorder.recorded()
    );
    state.drain_events();

    assert_eq!(
        recorder.recorded(),
        vec![
            RecordedCall::AddIntentToAdd(vec![PathBuf::from("new.txt")]),
            RecordedCall::ApplyPatch {
                direction: ApplyDirection::Forward
            },
        ],
        "stage on an Unversioned path must intent-to-add first, then forward-apply"
    );
    assert_eq!(
        porcelain_code(&repo.path, "new.txt"),
        "A ",
        "intent-to-add + patch leaves the file added to the index"
    );
}

/// A row click on an untracked file, which is the one case where the operation
/// still composes from the displayed value: an untracked path is in neither
/// index-facing patch, so the pane's synthesized creation diff is simultaneously
/// what the reader selected from and the only value git can take for that path.
/// The combined read shows a never-added file's content because the preview
/// synthesizes it, so there IS a row to click, and the row keeps its per-line
/// granularity through intent-to-add.
#[test]
fn a_row_click_on_an_untracked_file_stages_that_line_through_intent_to_add() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked-row");
    std::fs::write(repo.path.join("new.txt"), "one\ntwo\nthree\n").unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "new.txt");

    // Worktree line 2 is `two` in the synthesized one-hunk creation diff.
    granular::dispatch(
        &mut state,
        PathBuf::from("new.txt"),
        HunkTarget::Row(RowKind::Add, 0, 2),
    );
    state.drain_events();

    assert_eq!(
        recorder.recorded(),
        vec![
            RecordedCall::AddIntentToAdd(vec![PathBuf::from("new.txt")]),
            RecordedCall::ApplyPatch {
                direction: ApplyDirection::Forward
            },
        ],
        "the row click intents to add and then forward-applies exactly that line"
    );
    let staged = git(&repo.path, &["diff", "--cached", "--", "new.txt"]);
    assert!(
        staged.contains("+two") && !staged.contains("+three") && !staged.contains("+one"),
        "the clicked line alone reached the index:\n{staged}"
    );
    assert_eq!(
        porcelain_code(&repo.path, "new.txt"),
        "AM",
        "intent-to-add plus one staged line leaves the rest still unstaged"
    );
}

#[test]
fn granular_settle_excludes_fully_staged_file_and_advances_preview_order() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "settle");
    // words.txt: one-hunk worktree edit (Default bucket) — the op target.
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    std::fs::write(repo.path.join("words.txt"), BASE.replace("bravo", "BRAVO")).unwrap();
    // gone.txt: fully staged addition (Default bucket, rank 0) — pre-excluded
    // below to prove focus advance SKIPS exclusions.
    std::fs::write(repo.path.join("gone.txt"), "gone\n").unwrap();
    git(&repo.path, &["add", "gone.txt"]);
    // notes.txt: untracked (Unversioned bucket, rank 1) — expected successor.
    std::fs::write(repo.path.join("notes.txt"), "note\n").unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    state.ui.preview_change = Some(PathBuf::from("words.txt"));
    state
        .ui
        .granularly_completed
        .insert(repo.path.join("gone.txt"));
    seed_preview_cache(&mut state, &repo.path, "words.txt");

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );
    state.drain_events();

    // The whole file staged (single hunk) → story 9 exclusion lands under the
    // canonical (root-joined) key…
    assert!(
        state
            .ui
            .granularly_completed
            .contains(&repo.path.join("words.txt")),
        "fully staged file must be excluded, completed={:?}",
        state.ui.granularly_completed
    );
    // …and focus advances in display order Default → Unversioned, skipping
    // the excluded `gone.txt` (rank 0, would have won otherwise).
    assert_eq!(
        state.ui.preview_change,
        Some(PathBuf::from("notes.txt")),
        "focus must advance past exclusions to the next candidate"
    );
    assert!(
        recorder.recorded().contains(&RecordedCall::ApplyPatch {
            direction: ApplyDirection::Forward
        }),
        "the op itself must have been a forward apply, recorded={:?}",
        recorder.recorded()
    );
}

#[test]
fn granular_dispatch_without_cached_diff_is_a_silent_no_op() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "noop");
    seed_two_hunk_change(&repo);

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));

    // No `RootHunkStats` entry for the root, and no rendered preview: the
    // retained sections a granular op composes from are missing, and that is the
    // whole no-op. Fetching them would be a git spawn per palette verb, which is
    // what this contract exists to forbid.
    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );

    // The harness runs work inline, so a dispatched op would already have
    // reached the engine: nothing to wait for, and nothing arrived.
    state.drain_events();
    assert!(
        recorder.recorded().is_empty(),
        "missing inputs must not reach the engine, recorded={:?}",
        recorder.recorded()
    );
    assert!(
        state.ui.pending_granular.is_none(),
        "a no-op must not arm completion settlement"
    );
    assert!(!state.ui.busy, "a no-op must not mark the app busy");
}
