//! Granular op interface tests (spec R2 stories 3/8/9) — direct, headless.
//!
//! Exercises [`turbogit::core::granular::dispatch`] and — through its
//! production trigger, the `OpCompleted` → refresh → settle path in
//! [`turbogit::state::AppState::drain_events`] — the completion settlement
//! over a real temporary repository ([`AppState::for_roots`], sync_refresh
//! mode). No UI rendering: callers pass pure intent exactly as the diff
//! viewer's gutter controls and the palette verbs do, and assertions observe
//! **what reached the engine** (via [`common::RecordingExecutor`]) and the
//! resulting repository state via real `git` commands.

mod common;

use common::{RecordedCall, RecordingExecutor};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use turbogit::core::granular::{self, HunkTarget};
use turbogit::engine::{ApplyDirection, GitExecutor, cli::CliExecutor};
use turbogit::model::VcsSettings;
use turbogit::state::{AppState, DiffComparison};

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

/// The raw-diff cache key [`granular::dispatch`] resolves for `rel` under
/// working-tree chips (`Local`, no whitespace ignore) or the Staged
/// comparison — mirroring the module's own derivation so tests can seed
/// [`crate::state`] cache entries the ops will find.
fn preview_key(root: &Path, rel: &str, staged: bool) -> String {
    let left: Option<String> = None;
    let right: Option<String> = None;
    let path: Option<PathBuf> = Some(PathBuf::from(rel));
    let ws = false;
    format!("{root:?}|{left:?}|{right:?}|staged={staged}|ws={ws}|{path:?}")
}

/// Seed the viewer's raw-diff cache the way a rendered preview would have,
/// pinning the chip state the module's key derivation reads.
fn seed_preview_cache(state: &mut AppState, root: &Path, rel: &str, staged: bool, text: String) {
    state.ui.diff_comparison = if staged {
        DiffComparison::Staged
    } else {
        DiffComparison::Local
    };
    state.ui.diff_ignore_whitespace = false;
    state.ui.diff_cache = Some((preview_key(root, rel, staged), text));
}

/// Wait until the dispatched op's `OpCompleted` event has been drained —
/// which in sync_refresh mode also runs the synchronous refresh + granular
/// settlement — or panic once the deadline elapses.
fn pump(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_millis(15_000);
    while Instant::now() < deadline {
        state.drain_events();
        if !state.ui.busy && state.ui.pending_granular.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("granular op did not complete in time");
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: FnMut() -> bool>(ms: u64, mut f: F) -> bool {
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
fn granular_dispatch_stages_whole_hunk_forward() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "whole-hunk");
    seed_two_hunk_change(&repo);

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let diff_text = git(&repo.path, &["diff", "--", "words.txt"]);
    seed_preview_cache(&mut state, &repo.path, "words.txt", false, diff_text);

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        true,
    );

    assert!(
        wait_until(15_000, || porcelain_code(&repo.path, "words.txt") == "MM"),
        "staging hunk 0 must leave hunk 1 unstaged (partially staged MM)"
    );
    pump(&mut state);

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
    let diff_text = git(&repo.path, &["diff", "--", "words.txt"]);
    seed_preview_cache(&mut state, &repo.path, "words.txt", false, diff_text);

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
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
            staged.contains("+BRAVO")
        }),
        "the selected line must land in the index"
    );
    pump(&mut state);

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
fn granular_dispatch_unstages_hunk_via_reverse_apply() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "unstage");
    seed_two_hunk_change(&repo);
    // Fully stage the file first; the unstage op composes its patch from the
    // HEAD↔index diff, so the viewer would be showing the Staged comparison.
    git(&repo.path, &["add", "words.txt"]);

    let (mut state, recorder) =
        app_state_with_recorder(parent.path(), std::slice::from_ref(&repo.path));
    let diff_text = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
    seed_preview_cache(&mut state, &repo.path, "words.txt", true, diff_text);

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        false,
    );

    assert!(
        wait_until(15_000, || {
            let staged = git(&repo.path, &["diff", "--cached", "--", "words.txt"]);
            !staged.contains("BRAVO")
        }),
        "reverse-applying hunk 0 must remove it from the index"
    );
    pump(&mut state);

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
    // Creation diff in the exact shape the viewer synthesizes for untracked
    // previews (proven appliable by partial_stage_cli.rs).
    let creation_diff = concat!(
        "diff --git a/new.txt b/new.txt\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/new.txt\n",
        "@@ -0,0 +1,3 @@\n",
        "+one\n",
        "+two\n",
        "+three\n",
    );
    seed_preview_cache(
        &mut state,
        &repo.path,
        "new.txt",
        false,
        creation_diff.to_owned(),
    );

    granular::dispatch(
        &mut state,
        PathBuf::from("new.txt"),
        HunkTarget::Whole(0),
        true,
    );

    assert!(
        wait_until(15_000, || recorder.recorded().len() == 2),
        "untracked staging routes two engine calls, recorded={:?}",
        recorder.recorded()
    );
    pump(&mut state);

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
    let diff_text = git(&repo.path, &["diff", "--", "words.txt"]);
    seed_preview_cache(&mut state, &repo.path, "words.txt", false, diff_text);

    granular::dispatch(
        &mut state,
        PathBuf::from("words.txt"),
        HunkTarget::Whole(0),
        true,
    );
    pump(&mut state);

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

    std::thread::sleep(Duration::from_millis(300));
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
