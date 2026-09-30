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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::{RecordedCall, RecordingExecutor};

use turbogit_app::granular::{self, HunkTarget};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::state::{AppState, CharSelection, DiffComparison, Granularity};
use turbogit_domain::model::VcsSettings;
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

/// Populate the viewer's raw-diff cache the way a rendered preview would have:
/// pin the chip state, then ask the read for that target and settle its answer.
///
/// This used to spell the cache key out by hand so it could stuff a string into
/// the field — which pinned the very coupling this branch exists to remove: a
/// test that agrees with the module's key format by copying it proves nothing
/// when the two drift. Driving `read` asks the module instead, and the patch
/// text it answers with is the one `git diff` really produces for the fixture.
fn seed_preview_cache(state: &mut AppState, root: &Path, rel: &str, staged: bool) {
    state.ui.diff_comparison = if staged {
        DiffComparison::Staged
    } else {
        DiffComparison::Local
    };
    state.ui.diff_ignore_whitespace = false;
    let target = DiffTarget::new(
        root.to_path_buf(),
        None,
        None,
        state.ui.diff_comparison,
        false,
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
    seed_preview_cache(&mut state, &repo.path, "words.txt", false);

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        true,
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
fn granular_dispatch_stages_only_the_selected_lines_of_a_hunk() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "line-stage");
    seed_two_hunk_change(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "words.txt", false);

    // Story 3: sub-hunk selection — ordinal 1 of hunk 0 is the `+BRAVO`
    // addition (ordinals count +/- lines in order).
    state.ui.line_selections.insert(
        PathBuf::from("words.txt"),
        [(0usize, BTreeSet::from([1usize]))].into_iter().collect(),
    );

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Lines(0, BTreeSet::from([1])),
        true,
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
    seed_preview_cache(&mut state, &repo.path, "code.rs", false);

    granular::dispatch(
        &mut state,
        PathBuf::from("code.rs"),
        HunkTarget::Chars(0, 1, 8, 24),
        true,
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
fn switching_granularity_clears_accumulated_selections() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "granularity");
    seed_two_hunk_change(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    // Defaults preserve the pre-existing protocol: Line granularity, with
    // sub-hunk line toggling and a char drag both live.
    assert_eq!(state.ui.diff_granularity, Granularity::Line);
    state.ui.line_selections.insert(
        PathBuf::from("words.txt"),
        [(0usize, BTreeSet::from([1usize]))].into_iter().collect(),
    );
    state.ui.char_selection = Some(CharSelection {
        path: PathBuf::from("words.txt"),
        hunk: 0,
        ord: 1,
        start: 2,
        end: 5,
    });

    granular::set_granularity(&mut state, Granularity::Hunk);

    assert_eq!(state.ui.diff_granularity, Granularity::Hunk);
    assert!(
        state.ui.line_selections.is_empty(),
        "line selections refer to Line-mode semantics and must not survive \
         the switch"
    );
    assert!(
        state.ui.char_selection.is_none(),
        "a char selection must not survive the granularity switch"
    );
}

#[test]
fn escape_clears_the_active_char_selection() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "esc-clear");
    seed_two_hunk_change(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    state.ui.char_selection = Some(CharSelection {
        path: PathBuf::from("words.txt"),
        hunk: 0,
        ord: 1,
        start: 2,
        end: 5,
    });

    granular::clear_char_selection(&mut state);

    assert!(
        state.ui.char_selection.is_none(),
        "Esc clears the selection"
    );
}

#[test]
fn enter_stages_the_active_char_selection_and_consumes_it() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "enter-stage");
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
    seed_preview_cache(&mut state, &repo.path, "code.rs", false);
    state.ui.char_selection = Some(CharSelection {
        path: PathBuf::from("code.rs"),
        hunk: 0,
        ord: 1,
        start: 8,
        end: 24,
    });

    granular::stage_char_selection(&mut state);
    state.drain_events();

    let staged = git(&repo.path, &["diff", "--cached"]);
    assert!(
        staged.contains("+calculated_value\n") && !staged.contains("compute"),
        "Enter must stage exactly the selected bytes:\n{staged}"
    );
    assert!(
        state.ui.char_selection.is_none(),
        "staging consumes the char selection"
    );
    assert!(
        recorder.recorded().contains(&RecordedCall::ApplyPatch {
            direction: ApplyDirection::Forward
        }),
        "Enter routes through the same forward-apply seam, recorded={:?}",
        recorder.recorded()
    );
}

#[test]
fn file_granularity_dispatch_stages_every_hunk_of_the_diff() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "file-granularity");
    seed_two_hunk_change(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "words.txt", false);
    state.ui.diff_granularity = Granularity::File;

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::File,
        true,
    );
    state.drain_events();

    let staged = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
    assert!(
        staged.contains("+BRAVO") && staged.contains("+QUEBEC"),
        "File granularity must stage the whole file's diff:\n{staged}"
    );
    assert_eq!(
        porcelain_code(&repo.path, "words.txt"),
        "M ",
        "the file leaves no unstaged remainder behind"
    );
}

#[test]
fn selection_readout_reports_lines_chars_and_granularity() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "readout");
    seed_two_hunk_change(&repo);

    let (mut state, _recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    state.ui.preview_change = Some(PathBuf::from("words.txt"));

    // Nothing selected → no readout.
    assert!(granular::selection_readout(&state).is_none());

    // Two accumulated line ords → count + granularity, no char hints.
    state.ui.line_selections.insert(
        PathBuf::from("words.txt"),
        [(0usize, BTreeSet::from([1usize, 2usize]))]
            .into_iter()
            .collect(),
    );
    let readout = granular::selection_readout(&state).unwrap();
    assert!(
        readout.starts_with("2 line selections · granularity: line"),
        "line-only readout must report count and granularity, got: {readout}"
    );
    assert!(
        !readout.contains("chars"),
        "no chars without a char selection"
    );

    // One active char range of 16 chars → the dragged line joins the count,
    // the char count and the Enter/Esc hints appear.
    state.ui.char_selection = Some(CharSelection {
        path: PathBuf::from("words.txt"),
        hunk: 0,
        ord: 0,
        start: 8,
        end: 24,
    });
    let readout = granular::selection_readout(&state).unwrap();
    assert_eq!(
        readout,
        "3 line selections · 16 chars · granularity: line · ↵ stage · Esc clear"
    );

    // Singular wording for exactly one selected line.
    state.ui.line_selections.clear();
    let readout = granular::selection_readout(&state).unwrap();
    assert!(
        readout.starts_with("1 line selection · 16 chars"),
        "singular wording for one line, got: {readout}"
    );
}

#[test]
fn granular_dispatch_unstages_hunk_via_reverse_apply() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "unstage");
    seed_two_hunk_change(&repo);
    // Fully stage the file first; the unstage op composes its patch from the
    // HEAD↔index diff, so the viewer would be showing the Staged comparison.
    git(&repo.path, &["add", "words.txt"]);

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "words.txt", true);

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        false,
    );

    assert!(
        !git(&repo.path, &["diff", "--cached", "--", "words.txt"]).contains("BRAVO"),
        "reverse-applying hunk 0 must remove it from the index"
    );
    state.drain_events();

    assert!(
        recorder.recorded().contains(&RecordedCall::ApplyPatch {
            direction: ApplyDirection::Reverse
        }),
        "unstage must reverse-apply against the index, recorded={:?}",
        recorder.recorded()
    );
    let staged = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
    assert!(
        staged.contains("+QUEBEC"),
        "hunk 1 stays staged after unstaging hunk 0:\n{staged}"
    );
    assert_eq!(
        porcelain_code(&repo.path, "words.txt"),
        "MM",
        "index and worktree diverge again after the unstage"
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
    seed_preview_cache(&mut state, &repo.path, "new.txt", false);

    granular::dispatch(
        &mut state,
        PathBuf::from("new.txt"),
        HunkTarget::Whole(0),
        true,
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
    seed_preview_cache(&mut state, &repo.path, "words.txt", false);

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        true,
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
    assert_eq!(
        state.ui.diff_comparison,
        DiffComparison::Local,
        "post-op the viewer settles on the remaining unstaged changes"
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

    // No cache entry for the path (and never a render): every input the
    // module resolves besides the root is missing → silent no-op.
    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        true,
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
