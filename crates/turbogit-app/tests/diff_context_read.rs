//! The context read: one file section's text, asked for by a reveal and by
//! nothing else.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{commit, repo_with_one_commit};
use turbogit_app::diff_model::Hidden;
use turbogit_app::keyed_read::{ContextTarget, DiffTarget, Keyed, Read};
use turbogit_app::reveals::{Ask, Edge};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::DiffPolicies;
use turbogit_engine::fake::FakeExecutor;
use turbogit_engine_api::GitExecutor;

const ONE_SECTION: &str = "diff --git a/notes.txt b/notes.txt\n\
     index 1111111..2222222 100644\n\
     --- a/notes.txt\n\
     +++ b/notes.txt\n\
     @@ -7,7 +7,7 @@\n\
      line007\n\
      line008\n\
      line009\n\
     -line010\n\
     +LINE TEN\n\
      line011\n\
      line012\n\
      line013\n";

const DELETED_SECTION: &str = "diff --git a/gone.txt b/gone.txt\n\
     deleted file mode 100644\n\
     index 4cb29ea..0000000\n\
     --- a/gone.txt\n\
     +++ /dev/null\n\
     @@ -3,3 +0,0 @@\n\
     -three\n\
     -four\n\
     -five\n";

/// Twenty lines numbered exactly as `ONE_SECTION` numbers the context it prints,
/// so a revealed line and a git-printed line carry the same text and one fixture
/// answers for both.
fn notes_body() -> String {
    (1..=20).map(|n| format!("line{n:03}\n")).collect()
}

/// A repository whose tracked `notes.txt` holds all twenty of those lines, clean
/// against `HEAD`.
fn repo_with_notes(project: &Path) -> PathBuf {
    let work = repo_with_one_commit(project, "work");
    commit(&work, "notes.txt", &notes_body(), "seed");
    work
}

fn comparison(root: &Path, name: &str) -> DiffTarget {
    DiffTarget::new(
        root.to_path_buf(),
        None,
        None,
        DiffPolicies::default(),
        Some(PathBuf::from(name)),
    )
}

/// A state whose diff has settled from a canned patch, over a worktree the
/// caller's own bytes live in.
fn state_over(root: &Path, executor: FakeExecutor) -> AppState {
    let mut state = AppState::for_roots(root, std::slice::from_ref(&root.to_path_buf()))
        .with_executor(Arc::new(executor) as Arc<dyn GitExecutor>);
    while state.drain_events() > 0 {}
    let target = comparison(root, "notes.txt");
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    assert!(
        matches!(state.read(target), Read::Fresh(_)),
        "the canned patch settles first, because a context read resolves its \
         sides from the comparison it already holds"
    );
    state
}

fn fake_at(root: &Path, patch: &str) -> FakeExecutor {
    let mut fake = FakeExecutor::new();
    fake.diffs.insert(root.to_path_buf(), patch.to_owned());
    fake
}

fn settled_context(
    state: &mut AppState,
    comparison: &DiffTarget,
    file: usize,
) -> Read<Option<Arc<turbogit_app::reveals::SideText>>> {
    let key = comparison.key();
    state.ui.reveals.admit(&key, file);
    let target = ContextTarget::new(comparison.clone(), file);
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    state.read(target)
}

#[test]
fn opening_a_diff_asks_for_no_context_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_notes(tmp.path());
    let state = state_over(&work, fake_at(&work, ONE_SECTION));
    assert!(
        state
            .peek(ContextTarget::new(comparison(&work, "notes.txt"), 0))
            .is_none()
    );
    assert!(
        state.ui.context_side.is_none(),
        "the reveal text is what a tap asks for, never what an open asks for"
    );
    assert!(
        !state.read_pending(),
        "and nothing is mid-flight either: `for_roots` pumps inline, so any read \
         an open had dispatched would already have answered into the pad above"
    );
}

#[test]
fn a_revealed_line_is_the_unchanged_text_the_patch_never_printed() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_notes(tmp.path());
    std::fs::write(
        work.join("notes.txt"),
        notes_body().replace("line010\n", "LINE TEN\n"),
    )
    .unwrap();

    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    while state.drain_events() > 0 {}
    let target = comparison(&work, "notes.txt");
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    let Read::Fresh(value) = state.read(target.clone()) else {
        panic!("git's own answer settles");
    };
    let gaps = value.model.gaps();
    assert_eq!(
        gaps.gaps[0].hidden,
        Hidden::Run(6),
        "the real `-U3` leg leaves lines 1 to 6 unprinted"
    );

    let Read::Fresh(Some(text)) = settled_context(&mut state, &target, gaps.gaps[0].file) else {
        panic!("the worktree side settles as text");
    };
    assert_eq!(
        text.lines(),
        20,
        "the file's own length, which the patch lacks"
    );
    for line in 1..=6 {
        assert_eq!(
            text.line(line),
            Some(format!("line{line:03}").as_str()),
            "the revealed run is the unchanged text, numbered the way the gutter prints it"
        );
    }
    assert_eq!(
        text.line(7),
        Some("line007"),
        "and it meets the patch mid-line"
    );
    assert_eq!(text.line(21), None, "past the end is nothing, not a panic");
    assert!(
        matches!(state.ui.reveals.side(&target.key(), 0), Ask::Have(have)
            if have.lines() == 20),
        "the fold landed the same text the read answered with"
    );
}

#[test]
fn a_deleted_file_reads_the_one_side_it_still_has() {
    let tmp = tempfile::tempdir().unwrap();
    // `gone.txt` is in no side of this worktree, so the diff comes from the
    // canned section and the old side is the only text the read can reach.
    let work = repo_with_one_commit(tmp.path(), "work");
    let mut fake = FakeExecutor::new();
    fake.diffs.insert(work.clone(), DELETED_SECTION.to_owned());
    fake.files_bytes
        .lock()
        .unwrap()
        .insert(PathBuf::from("gone.txt"), b"one\ntwo\nthree\n".to_vec());
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work))
        .with_executor(Arc::new(fake) as Arc<dyn GitExecutor>);
    while state.drain_events() > 0 {}
    let target = DiffTarget::new(
        work,
        Some("HEAD".to_owned()),
        None,
        DiffPolicies::default(),
        Some(PathBuf::from("gone.txt")),
    );
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    assert!(matches!(state.read(target.clone()), Read::Fresh(_)));

    let Read::Fresh(Some(text)) = settled_context(&mut state, &target, 0) else {
        panic!("the surviving side is the old one, and it is readable");
    };
    assert_eq!(text.line(3), Some("three"));
    assert_eq!(
        text.line(10),
        None,
        "the read is the file, not the hunk: three lines is three lines"
    );
}

#[test]
fn bytes_that_are_not_utf8_settle_to_no_text() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_notes(tmp.path());
    std::fs::write(work.join("notes.txt"), [b'x', 0xff, 0xfe, b'y']).unwrap();
    let mut state = state_over(&work, fake_at(&work, ONE_SECTION));

    let target = comparison(&work, "notes.txt");
    let key = target.key();
    state.ui.reveals.admit(&key, 0);
    assert!(matches!(
        state.read(ContextTarget::new(target.clone(), 0)),
        Read::Waiting
    ));
    state.drain_events();
    assert!(
        matches!(
            state.read(ContextTarget::new(target.clone(), 0)),
            Read::Empty
        ),
        "a refused side is an answer of nothing, not a failure to retry"
    );
    assert!(
        matches!(state.ui.reveals.side(&key, 0), Ask::None),
        "and the reveal is not left waiting for text that will never land"
    );

    std::fs::write(work.join("notes.txt"), notes_body()).unwrap();
    state.refresh(Affected::All);
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    let Read::Fresh(Some(text)) = settled_context(&mut state, &target, 0) else {
        panic!("the refusal is the bytes' own: the same side answers as text once it decodes");
    };
    assert_eq!(
        text.line(1),
        Some("line001"),
        "and it is the file the read answered from"
    );
}

#[test]
fn counts_are_intent_for_one_comparison_and_survive_its_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_notes(tmp.path());
    let mut state = state_over(&work, fake_at(&work, ONE_SECTION));
    let target = comparison(&work, "notes.txt");
    let key = target.key();

    state.ui.reveals.admit(&key, 0);
    assert!(matches!(
        state.read(ContextTarget::new(target.clone(), 0)),
        Read::Waiting
    ));
    state.drain_events();
    assert!(matches!(
        state.read(ContextTarget::new(target.clone(), 0)),
        Read::Fresh(Some(_))
    ));

    state.ui.reveals.tap(&key, 0, Edge::Bottom, Some(40));
    state.refresh(Affected::All);
    assert_eq!(
        state.ui.reveals.edges(&key, 0),
        [0, 10],
        "staging a line is a refresh, and a refresh that answers the same \
         comparison must not close what the user opened"
    );
    assert!(
        state.ui.context_side.is_none(),
        "the landing pad is retired with every other cached value"
    );
    assert!(
        matches!(state.ui.reveals.side(&key, 0), Ask::Have(_)),
        "the text a region was painted from lives in the reveal, not in the pad, \
         so eviction between the two surfaces cannot blank it"
    );
    assert_eq!(state.ui.reveals.edges("another|comparison", 0), [0, 0]);
}
