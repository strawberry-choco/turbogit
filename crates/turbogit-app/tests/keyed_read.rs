use std::path::{Path, PathBuf};

use test_support::git_seed::git;
use turbogit_app::diff_model::FileMeta;
use turbogit_app::keyed_read::{DiffTarget, PaneFlavour, PaneTarget, Read};
use turbogit_app::state::{AppState, BlameTarget};
use turbogit_domain::model::{Patch, PatchHeaderLine, PatchLineKind};

fn body_lines(patch: &Patch) -> impl Iterator<Item = &turbogit_domain::model::PatchLine> {
    patch
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.lines)
}

fn repo(dir: &Path, dirty: bool) -> PathBuf {
    let work = test_support::git_seed::repo_with_one_commit(dir, "work");
    if dirty {
        std::fs::write(work.join("README.md"), "X\nthree\n").unwrap();
    }
    work
}

fn repo_with_staged_and_unstaged(dir: &Path) -> PathBuf {
    let work = test_support::git_seed::repo_with_one_commit(dir, "both");
    std::fs::write(work.join("README.md"), "TWO\n").unwrap();
    git(&work, &["add", "."]);
    std::fs::write(work.join("README.md"), "TWO\nthree\n").unwrap();
    work
}

fn working_tree_read(work: &Path) -> DiffTarget {
    DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        turbogit_domain::model::DiffPolicies::default(),
        Some(PathBuf::from("README.md")),
    )
}

fn settle_first(state: &mut AppState) {
    while state.drain_events() > 0 {}
}

#[test]
fn a_cold_read_waits_and_the_next_one_answers_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), true);
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    assert!(
        matches!(state.read(working_tree_read(&work)), Read::Waiting),
        "nothing is cached for the target yet, so the first ask waits"
    );
    state.drain_events();

    let Read::Fresh(diff) = state.read(working_tree_read(&work)) else {
        panic!("the settled answer must be the one the target asked for");
    };
    assert!(
        diff.patch.files[0].headers.iter().any(|h| matches!(
            h,
            PatchHeaderLine::Sources { new, .. } if new == "b/README.md"
        )),
        "the value carries its source pair:\n{}",
        diff.patch
    );
    assert_eq!(diff.model.hunk_count(), 1, "and the model built from it");
}

#[test]
fn one_frame_asks_for_a_diff_once() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), true);
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let target = working_tree_read(&work);
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    assert!(
        matches!(state.read(target.clone()), Read::Waiting),
        "a second ask while one is in flight waits rather than spending a second fetch"
    );

    assert_eq!(
        state.drain_events(),
        1,
        "two reads in one frame admitted one fetch"
    );
}

#[test]
fn a_settled_empty_comparison_answers_empty_not_waiting() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), false);
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    assert!(matches!(
        state.read(working_tree_read(&work)),
        Read::Waiting
    ));
    state.drain_events();
    assert!(
        matches!(state.read(working_tree_read(&work)), Read::Empty),
        "a clean file has a settled answer of nothing to show"
    );

    assert_eq!(
        state.drain_events(),
        0,
        "an empty answer does not re-dispatch every frame"
    );
}

#[test]
fn the_pinned_read_answers_with_git_diffs_content() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_staged_and_unstaged(tmp.path());
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    assert!(matches!(
        state.read(working_tree_read(&work)),
        Read::Waiting
    ));
    state.drain_events();
    let Read::Fresh(diff) = state.read(working_tree_read(&work)) else {
        panic!("the pinned working-tree read settles");
    };
    assert!(
        body_lines(&diff.patch).any(|l| l.kind == PatchLineKind::Added && l.text == "three"),
        "index vs worktree reaches the unstaged edit:\n{}",
        diff.patch
    );
    assert!(
        body_lines(&diff.patch).all(|l| l.text != "x"),
        "HEAD's line belongs to a HEAD-relative pair, so its absence is what says \
         this read is not one:\n{}",
        diff.patch
    );
    assert!(
        body_lines(&diff.patch)
            .filter(|l| l.kind == PatchLineKind::Added)
            .all(|l| l.text != "TWO"),
        "the staged edit is this read's old side, never its addition:\n{}",
        diff.patch
    );

    let plain = git(&work, &["diff", "--", "README.md"]);
    let staged = git(&work, &["diff", "--cached", "--", "README.md"]);
    assert!(
        plain.contains("+three") && !staged.contains("+three"),
        "the fixture must keep discriminating the two diffs, got:\n{plain}\n---\n{staged}"
    );
}

#[test]
fn one_read_per_file_while_nothing_about_it_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_staged_and_unstaged(tmp.path());
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let target = working_tree_read(&work);
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    assert_eq!(
        state.drain_events(),
        1,
        "the first ask spent exactly one diff fetch"
    );

    for _ in 0..3 {
        assert!(
            matches!(state.read(target.clone()), Read::Fresh(_)),
            "the same key keeps answering, so no view change can be hiding behind it"
        );
        assert_eq!(
            state.drain_events(),
            0,
            "and nothing about the file changed, so nothing was dispatched"
        );
    }
}

#[test]
fn a_pane_answers_with_both_sides_and_keeps_them() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), false);
    let png = png_bytes();
    std::fs::write(work.join("art.png"), &png).unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "art"]);
    std::fs::write(work.join("art.png"), [png.clone(), vec![0u8; 64]].concat()).unwrap();

    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let comparison = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        turbogit_domain::model::DiffPolicies::default(),
        Some(PathBuf::from("art.png")),
    );
    let file = FileMeta {
        old_path: Some("art.png".into()),
        new_path: Some("art.png".into()),
        binary: true,
        ..FileMeta::default()
    };
    let art = PaneTarget::new(comparison.clone(), PaneFlavour::Binary, file.clone(), false);
    assert!(
        matches!(state.read(art.clone()), Read::Waiting),
        "the first ask for a pane's bytes waits"
    );
    state.drain_events();
    let Read::Fresh(entry) = state.read(art.clone()) else {
        panic!("both sides of the pane settle");
    };
    let before = entry.old.as_ref().expect("index side").byte_len;
    let after = entry.new.as_ref().expect("worktree side").byte_len;
    assert_eq!(
        before,
        png.len() as u64,
        "the index side is what was staged"
    );
    assert_eq!(after, png.len() as u64 + 64, "the worktree side grew");

    let image = PaneTarget::new(comparison, PaneFlavour::Image, file, true);
    assert!(matches!(state.read(image.clone()), Read::Waiting));
    state.drain_events();
    assert!(matches!(state.read(image), Read::Fresh(_)));
    assert!(
        matches!(state.read(art), Read::Fresh(_)),
        "the keyed map still holds the pane that was here before"
    );
}

fn png_bytes() -> Vec<u8> {
    let img = image::DynamicImage::new_rgb8(2, 3);
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[test]
fn peek_answers_without_admitting() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), true);
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let target = working_tree_read(&work);
    assert!(
        state.peek(target.clone()).is_none(),
        "a cold slot answers nothing at all"
    );
    assert_eq!(
        state.drain_events(),
        0,
        "and asking it that way spent no git work"
    );

    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    let peeked = state.peek(target).expect("the read settled one");
    assert_eq!(peeked.model.hunk_count(), 1);
}

#[test]
fn a_synthesized_answer_leaves_nothing_pending() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), false);
    std::fs::write(work.join("brand-new.txt"), "fresh\n").unwrap();
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let target = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        turbogit_domain::model::DiffPolicies::default(),
        Some(PathBuf::from("brand-new.txt")),
    );
    state.read(target.clone());
    let Read::Fresh(diff) = state.read(target) else {
        panic!("an untracked file's creation diff is synthesized, not fetched");
    };
    assert!(
        diff.patch.files[0]
            .headers
            .iter()
            .any(|h| matches!(h, PatchHeaderLine::Sources { new, .. } if new == "b/brand-new.txt")),
        "the whole file reads as an addition:\n{}",
        diff.patch
    );
    assert!(
        !state.read_pending(),
        "a value answered without a worker must not hold the read open"
    );
}

#[test]
fn a_blame_answer_is_not_another_revisions_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let work = test_support::git_seed::repo_with_one_commit(tmp.path(), "blame");
    let first = git(&work, &["rev-parse", "HEAD"]).trim().to_string();
    test_support::git_seed::commit(&work, "README.md", "one\ntwo\n", "second");
    let second = git(&work, &["rev-parse", "HEAD"]).trim().to_string();

    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);
    let root = state.selected_root.clone().unwrap();
    let blame_at = |rev: &str| BlameTarget {
        root: root.clone(),
        path: PathBuf::from("README.md"),
        rev: rev.to_owned(),
    };

    let at_second = blame_at(&second);
    assert!(matches!(state.read(at_second.clone()), Read::Waiting));
    state.drain_events();
    let lines = match state.read(at_second.clone()) {
        Read::Fresh(lines) => lines,
        _ => panic!("the blame answer settles"),
    };
    assert_eq!(lines.len(), 2, "two lines blamed at the second commit");

    let at_first = blame_at(&first);
    assert!(
        matches!(state.read(at_first.clone()), Read::Waiting),
        "the other revision's read waits rather than answering with these lines"
    );
    state.drain_events();
    let Read::Fresh(lines) = state.read(at_first) else {
        panic!("its own answer settles");
    };
    assert_eq!(lines.len(), 1, "one line existed at the first commit");
    assert!(
        matches!(state.read(at_second), Read::Waiting),
        "blame stores one entry, so switching revisions back refetches — the
        policy the keyed pane-bytes map deliberately does not share"
    );
}
