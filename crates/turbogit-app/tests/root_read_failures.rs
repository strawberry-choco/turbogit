//! The recorded failure of a root-wide diff read, graded against real git.
//!
//! `ensure_hunk_stats` asks one root for three whole-root diffs. When one of
//! those reads dies, the fill now records the failure against that root and
//! that view instead of swallowing it, and a surface displaying that view
//! refuses: the whole file counts as failed to read, and only the message
//! shows. Every fixture is a temporary repository driven by the CLI executor
//! through `AppState::for_roots`, per `AGENTS.md`, and every claim about the
//! index is read back out of `git status --porcelain` or `git diff --cached`,
//! not out of an internal flag.
//!
//! The natural failing read is the `HEAD` one on a repository nobody has
//! committed in: the engine's `guard_unborn_head` refuses it with
//! `TgError::NoCommitsYet` while both index-facing reads answer. That is also
//! the partially failed fill the module doc describes, so these tests do not
//! need a double that can fail `diff_patch`: `FakeExecutor` answers `Ok`
//! always, and none is extended here. The one case the unborn fixture cannot
//! produce, a fresh per-path answer over a recorded root failure, is produced
//! by recording the failure directly on the caches, which is the event-fed
//! writer's own public door.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{git, pin_eol_lf};
use turbogit_app::diff_model::{DiffValue, DisplayRow, RowKind};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::root_caches::{Affected, RootCaches, RootHunkStats, StatsView};
use turbogit_app::staged_lines;
use turbogit_app::state::AppState;
use turbogit_domain::model::{DiffPolicies, RootId};

const NO_COMMITS_MESSAGE: &str =
    "this repository has no commits yet: there is no first commit to compare your changes against";

fn root_id(repo: &Path) -> RootId {
    RootId(repo.to_path_buf().into())
}

fn app_state(project: &Path, repo: &Path) -> AppState {
    AppState::for_roots(project, std::slice::from_ref(&repo.to_path_buf()))
}

/// A repository with `git init` and nothing else: no commit, no HEAD.
fn unborn_repo(project: &Path, name: &str) -> PathBuf {
    let work = project.join(name);
    git(
        project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    pin_eol_lf(&work);
    work
}

/// Fill `repo`'s hunk statistics the way the app fills them, on ask.
fn fill(state: &mut AppState, repo: &Path) {
    let root = root_id(repo);
    state.ensure_hunk_stats(&root);
}

/// What the fill installed, without filling anything.
fn held<'a>(state: &'a AppState, repo: &Path) -> &'a RootHunkStats {
    state
        .caches
        .hunk_stats(&root_id(repo))
        .expect("the root's entry fills on ask")
}

fn preview_target(repo: &Path, rel: &str) -> DiffTarget {
    DiffTarget::working_tree(
        repo.to_path_buf(),
        DiffPolicies::default(),
        Some(PathBuf::from(rel)),
    )
}

/// Ask, settle on the pump, ask again: the read's own two-frame life.
fn read_value(state: &mut AppState, repo: &Path, rel: &str) -> Arc<DiffValue> {
    let target = preview_target(repo, rel);
    for _ in 0..6 {
        if let Read::Fresh(value) = state.read(target.clone()) {
            return value;
        }
        state.drain_events();
    }
    panic!("the diff read never settles for {rel}");
}

/// The changed rows of one display model as `(kind, text)`, sorted by text so
/// the assertion prints stably.
fn changed(model: &DiffValue) -> Vec<(RowKind, String)> {
    let mut out: Vec<(RowKind, String)> = model
        .model
        .display
        .iter()
        .flat_map(|row| match row {
            DisplayRow::Full(r) => vec![r],
            DisplayRow::Pair(a, b) => a.iter().chain(b.iter()).collect(),
        })
        .filter(|r| matches!(r.kind, RowKind::Del | RowKind::Add))
        .map(|r| (r.kind, r.text.clone()))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// **The partially failed fill, in its natural shape.** An unborn root fails
/// only the HEAD-facing read; `git diff --cached` and `git diff` answer, and
/// the data they feed must stay on screen while the failed view is refused.
/// Git's own three answers are asserted first, because the shape of this
/// fixture is the premise of every other test here.
#[test]
fn a_failed_head_read_marks_only_its_view_and_the_answered_views_still_fill() {
    let parent = tempfile::tempdir().unwrap();
    let repo = unborn_repo(parent.path(), "unborn-partial");
    std::fs::write(repo.join("f.txt"), "alpha\nbravo\n").unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(repo.join("f.txt"), "alpha\nbravo\ncharlie\n").unwrap();

    assert_eq!(
        git(&repo, &["status", "--porcelain", "--", "f.txt"]).trim_end(),
        "AM f.txt",
        "git agrees the file is both staged and further edited, so the two \
         index-facing reads have real content"
    );
    assert!(
        git(&repo, &["diff", "--cached", "--", "f.txt"]).contains("@@ -0,0 +1,2 @@"),
        "and `git diff --cached` answers on an unborn root"
    );
    assert!(
        git(&repo, &["diff", "--", "f.txt"]).contains("+charlie"),
        "and so does plain `git diff`"
    );

    let mut state = app_state(parent.path(), &repo);
    let root = root_id(&repo);
    fill(&mut state, &repo);
    let stats = held(&state, &repo);

    assert_eq!(
        state.caches.diff_view_failure(&root, StatsView::Repo),
        Some(NO_COMMITS_MESSAGE),
        "the read that died is recorded, with the engine's own sentence"
    );
    for view in [StatsView::Staged, StatsView::Local] {
        assert!(
            !state.caches.has_diff_view_failure(&root, view),
            "the reads that answered record nothing: {view:?}"
        );
    }

    assert!(
        stats.file(StatsView::Repo, Path::new("f.txt")).is_none(),
        "the failed view contributes an empty list, not a fake answer"
    );
    assert!(
        stats
            .file(StatsView::Staged, Path::new("f.txt"))
            .is_some_and(|f| !f.hunks.is_empty()),
        "the badge and chip-rail data that comes from the staged read is real \
         and must not be blanked by the dead HEAD read"
    );
    assert!(
        stats
            .file(StatsView::Local, Path::new("f.txt"))
            .is_some_and(|f| !f.hunks.is_empty()),
        "and the same for the unstaged half"
    );
    let sets = staged_lines::find(&stats.line_sets, Path::new("f.txt")).expect("the file");
    assert_eq!(
        (
            sets.staged.added.runs(),
            sets.staged.removed.runs(),
            sets.local.added.runs(),
            sets.local.removed.runs(),
        ),
        (
            &[(1usize, 2usize)][..],
            &[][..],
            &[(3usize, 3usize)][..],
            &[][..]
        ),
        "the per-line oracle keeps the data the two answered reads derived: {sets:?}"
    );

    assert_eq!(
        state
            .failed_view_message(&preview_target(&repo, "f.txt"))
            .as_deref(),
        Some(NO_COMMITS_MESSAGE),
        "and the pane rule marks the displayed file as failed to read"
    );
    let commit_pair = DiffTarget::new(
        repo.clone(),
        Some("aaa1111".to_owned()),
        Some("bbb2222".to_owned()),
        DiffPolicies::default(),
        Some(PathBuf::from("f.txt")),
    );
    assert!(
        state.failed_view_message(&commit_pair).is_none(),
        "a commit-to-commit target names its own revisions; a failed root \
         read says nothing about them"
    );
}

/// The other state of the same rule: an unversioned path is exempt because
/// its pane never asked git the question that failed, while the whole-root
/// pane is not exempt at all.
#[test]
fn an_unversioned_pane_is_exempt_and_the_whole_root_pane_is_not() {
    let parent = tempfile::tempdir().unwrap();
    let repo = unborn_repo(parent.path(), "unborn-unversioned");
    std::fs::write(repo.join("scratch.txt"), "untracked\n").unwrap();

    let mut state = app_state(parent.path(), &repo);
    let root = root_id(&repo);
    fill(&mut state, &repo);
    assert!(
        state.caches.has_diff_view_failure(&root, StatsView::Repo),
        "the fill still records the dead read for the root"
    );

    assert!(
        state
            .failed_view_message(&preview_target(&repo, "scratch.txt"))
            .is_none(),
        "the unversioned pane's answer is synthesized from the file's bytes \
         and its staging route composes from that value, so its controls are \
         not silently inert and the file is not marked unreadable"
    );
    let whole_root = DiffTarget::working_tree(repo.clone(), DiffPolicies::default(), None);
    assert_eq!(
        state.failed_view_message(&whole_root).as_deref(),
        Some(NO_COMMITS_MESSAGE),
        "a path-less target displays the failed view with no exemption and \
         must show the message"
    );
}

/// The failure is not sticky: the same read answering forgets it, and the
/// file becomes readable after the first commit. This is the non-sticky
/// property the keyed path established for the pane read, and the fill's path
/// must match it, because every completed operation invalidates the root and
/// the refill is what clears the record.
#[test]
fn the_record_clears_when_the_same_read_answers_after_the_first_commit() {
    let parent = tempfile::tempdir().unwrap();
    let repo = unborn_repo(parent.path(), "unborn-clears");
    std::fs::write(repo.join("f.txt"), "alpha\nbravo\n").unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(parent.path(), &repo);
    let root = root_id(&repo);
    fill(&mut state, &repo);
    assert!(state.caches.has_diff_view_failure(&root, StatsView::Repo));

    git(&repo, &["commit", "-q", "-m", "first"]);
    std::fs::write(repo.join("f.txt"), "alpha\nbravo\ncharlie\n").unwrap();
    // What a completed operation's refresh does for the root: the entry and
    // its records go together, so the next fill asks git again.
    state.refresh(Affected::Root(root.clone()));
    assert!(
        !state.caches.has_diff_view_failure(&root, StatsView::Repo),
        "invalidating the root drops its recorded failures with its entry"
    );

    fill(&mut state, &repo);
    assert!(
        !state.caches.has_diff_view_failure(&root, StatsView::Repo),
        "and the read that now answers leaves no record standing"
    );
    assert!(
        state.caches.hunk_stats(&root).is_some_and(|s| s
            .file(StatsView::Repo, Path::new("f.txt"))
            .is_some_and(|f| !f.hunks.is_empty())),
        "the HEAD view fills for real once HEAD exists"
    );
    assert!(
        state
            .failed_view_message(&preview_target(&repo, "f.txt"))
            .is_none(),
        "so the pane rule releases the file"
    );

    let value = read_value(&mut state, &repo, "f.txt");
    assert_eq!(
        changed(&value),
        [(RowKind::Add, "charlie".to_owned())],
        "the same target answers rows through the production read"
    );
    assert_eq!(
        git(&repo, &["status", "--porcelain", "--", "f.txt"]).trim_end(),
        " M f.txt",
        "and git agrees about the state the pane now shows"
    );
}

/// The invalidation arms, one test with teeth on all three: `is_empty` must
/// enumerate the new map or it can assert a leak-free cache while a failure
/// leaks forever; the per-root retain must drop the root's records and no
/// other root's; `invalidate_all` must clear the map with the rest.
#[test]
fn the_failure_map_lives_under_the_same_invalidation_as_the_entry() {
    let mut caches = RootCaches::default();
    let first = RootId(PathBuf::from("/roots/first").into());
    let second = RootId(PathBuf::from("/roots/second").into());
    caches.record_diff_view_failure(&first, StatsView::Repo, "the HEAD read died");
    caches.record_diff_view_failure(&second, StatsView::Staged, "the staged read died");

    assert!(
        !caches.is_empty(),
        "a recorded diff-view failure is cache state: `is_empty` is the \
         invalidation invariant's diagnostic, and a new map it does not \
         enumerate is an assertion that can pass while a failure leaks forever"
    );

    caches.invalidate(&Affected::Root(first.clone()));
    assert!(
        !caches.has_diff_view_failure(&first, StatsView::Repo),
        "a root's diff-view failures go with its entry"
    );
    assert!(
        caches.has_diff_view_failure(&second, StatsView::Staged),
        "and a surface-wide or cross-root drop is exactly what the per-read \
         rule refuses: the other root keeps its record"
    );

    caches.invalidate_all();
    assert!(
        caches.is_empty(),
        "the whole-cache clear has an arm for the new map"
    );
}

/// The case the unborn fixture cannot produce: the per-path read answers with
/// real content while the root read of the same view is recorded as failed.
/// (No existing double can make `diff_patch` answer an error at all, so the
/// record is written through the caches' own event-fed door against a
/// committed repository whose reads would otherwise all answer.) The rule
/// marks the whole file failed to read even against a `Fresh` answer: content
/// the composition path cannot honour is not shown at all.
#[test]
fn a_recorded_view_failure_refuses_even_a_fresh_per_path_answer() {
    let parent = tempfile::tempdir().unwrap();
    let work = parent.path().join("committed");
    git(
        parent.path(),
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    pin_eol_lf(&work);
    std::fs::write(work.join("f.txt"), "alpha\nbeta\n").unwrap();
    git(&work, &["add", "--", "f.txt"]);
    git(&work, &["commit", "-q", "-m", "init"]);
    std::fs::write(work.join("f.txt"), "alpha\nBETA\n").unwrap();

    let mut state = app_state(parent.path(), &work);
    let root = root_id(&work);
    let value = read_value(&mut state, &work, "f.txt");
    assert_eq!(
        changed(&value),
        [
            (RowKind::Add, "BETA".to_owned()),
            (RowKind::Del, "beta".to_owned())
        ],
        "the fixture's premise: the per-path read answers with rows"
    );
    assert!(
        state
            .failed_view_message(&preview_target(&work, "f.txt"))
            .is_none(),
        "and with nothing recorded, the pane keeps them"
    );

    state
        .caches
        .record_diff_view_failure(&root, StatsView::Repo, "the whole-root HEAD read died");
    assert_eq!(
        state
            .failed_view_message(&preview_target(&work, "f.txt"))
            .as_deref(),
        Some("the whole-root HEAD read died"),
        "recording the failure marks the file unreadable over a live answer, \
         which is the only window where an inert painted row could appear"
    );
    let local_target = DiffTarget::new(
        work.clone(),
        None,
        None,
        DiffPolicies::default(),
        Some(PathBuf::from("f.txt")),
    );
    assert!(
        state.failed_view_message(&local_target).is_none(),
        "the other views are not blacked out by the Repo record"
    );
}
