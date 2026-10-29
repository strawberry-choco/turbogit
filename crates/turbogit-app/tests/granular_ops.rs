use std::path::{Path, PathBuf};
use std::sync::Arc;
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::{RecordedCall, RecordingExecutor};

use turbogit_app::diff_model::RowKind;
use turbogit_app::granular::{self, Direction, HunkTarget};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::{ApplyDirection, GitExecutor};

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

fn temp_repo(parent: &Path, name: &str) -> Repo {
    Repo {
        path: repo_with_one_commit(parent, name),
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

fn change(repo: &Path, rel: &str) -> ChangeRef {
    ChangeRef::new(RootId(repo.to_path_buf().into()), rel)
}

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
        change(&repo.path, "words.txt"),
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

    granular::dispatch(
        &mut state,
        change(&repo.path, "words.txt"),
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
        change(&repo.path, "code.rs"),
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
    let change = Some(change(&repo.path, "code.rs"));

    granular::begin_char_drag(&mut state, &change, 0, 1, 2, 8);
    granular::extend_char_drag(&mut state, &change, 0, 1, 24);
    assert!(
        granular::finish_char_drag(&mut state, &change, 0, 1),
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

    granular::begin_char_drag(&mut state, &change, 0, 0, 0, 4);
    let staged_before = git(&repo.path, &["diff", "--cached"]);
    assert!(
        !granular::finish_char_drag(&mut state, &change, 0, 0),
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
    std::fs::write(repo.path.join("new.txt"), "one\ntwo\nthree\n").unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "new.txt");

    granular::dispatch(
        &mut state,
        change(&repo.path, "new.txt"),
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

#[test]
fn a_row_click_on_an_untracked_file_stages_that_line_through_intent_to_add() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked-row");
    std::fs::write(repo.path.join("new.txt"), "one\ntwo\nthree\n").unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    seed_preview_cache(&mut state, &repo.path, "new.txt");

    granular::dispatch(
        &mut state,
        change(&repo.path, "new.txt"),
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
    std::fs::write(repo.path.join("words.txt"), BASE).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
    std::fs::write(repo.path.join("words.txt"), BASE.replace("bravo", "BRAVO")).unwrap();
    std::fs::write(repo.path.join("gone.txt"), "gone\n").unwrap();
    git(&repo.path, &["add", "gone.txt"]);
    std::fs::write(repo.path.join("notes.txt"), "note\n").unwrap();

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    state.ui.preview_change = Some(change(&repo.path, "words.txt"));
    state
        .ui
        .granularly_completed
        .insert(change(&repo.path, "gone.txt"));
    seed_preview_cache(&mut state, &repo.path, "words.txt");

    granular::dispatch(
        &mut state,
        change(&repo.path, "words.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );
    state.drain_events();

    assert!(
        state
            .ui
            .granularly_completed
            .contains(&change(&repo.path, "words.txt")),
        "fully staged file must be excluded, completed={:?}",
        state.ui.granularly_completed
    );
    assert_eq!(
        state.ui.preview_change,
        Some(change(&repo.path, "notes.txt")),
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

    granular::dispatch(
        &mut state,
        change(&repo.path, "words.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );

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
