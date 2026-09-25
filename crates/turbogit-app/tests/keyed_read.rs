//! The keyed read at its own interface (ADR-0021): a target goes in, a verdict
//! comes back, and asking is also what starts getting the answer.
//!
//! Nothing here reaches into a cache field or names a key — the point of the
//! read is that the staleness rule is only written in one place, so it is only
//! ever tested in one place too. The headless harness runs git work inline, so
//! a fetch's answer is already in the channel when the read returns and one
//! `drain_events` settles it; no thread and no deadline is involved.

use std::path::{Path, PathBuf};
use std::process::Command;

use turbogit_app::diff_model::FileMeta;
use turbogit_app::keyed_read::{DiffTarget, PaneFlavour, PaneTarget, Read};
use turbogit_app::state::{AppState, BlameTarget, DiffComparison};
use turbogit_domain::model::{Patch, PatchHeaderLine, PatchLineKind};

/// Every body line the patch holds, whatever side it belongs to.
fn body_lines(patch: &Patch) -> impl Iterator<Item = &turbogit_domain::model::PatchLine> {
    patch
        .files
        .iter()
        .flat_map(|f| &f.hunks)
        .flat_map(|h| &h.lines)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// One committed `README.md`, optionally left with an unstaged edit.
fn repo(dir: &Path, dirty: bool) -> PathBuf {
    let work = dir.join("work");
    git(dir, &["init", "-q", "-b", "main", work.to_str().unwrap()]);
    std::fs::write(work.join("README.md"), "one\ntwo\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    if dirty {
        std::fs::write(work.join("README.md"), "one\nTWO\nthree\n").unwrap();
    }
    work
}

/// A `README.md` with a staged edit *and* a further unstaged one on top, so
/// the Staged and Repo comparisons have genuinely different answers to reach.
fn repo_with_staged_and_unstaged(dir: &Path) -> PathBuf {
    let work = dir.join("both");
    git(dir, &["init", "-q", "-b", "main", work.to_str().unwrap()]);
    std::fs::write(work.join("README.md"), "one\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    std::fs::write(work.join("README.md"), "TWO\n").unwrap();
    git(&work, &["add", "."]);
    std::fs::write(work.join("README.md"), "TWO\nthree\n").unwrap();
    work
}

fn head_vs_worktree(work: &Path) -> DiffTarget {
    DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        DiffComparison::Repo,
        false,
        Some(PathBuf::from("README.md")),
    )
}

/// Settle everything the launch queued, so an event count below means exactly
/// what one read put there.
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
        matches!(state.read(head_vs_worktree(&work)), Read::Waiting),
        "nothing is cached for the target yet, so the first ask waits"
    );
    state.drain_events();

    let Read::Fresh(diff) = state.read(head_vs_worktree(&work)) else {
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

    let target = head_vs_worktree(&work);
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

    assert!(matches!(state.read(head_vs_worktree(&work)), Read::Waiting));
    state.drain_events();
    assert!(
        matches!(state.read(head_vs_worktree(&work)), Read::Empty),
        "a clean file has a settled answer of nothing to show"
    );

    assert_eq!(
        state.drain_events(),
        0,
        "an empty answer does not re-dispatch every frame"
    );
}

#[test]
fn a_different_comparison_is_not_answered_by_the_cached_one() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_staged_and_unstaged(tmp.path());
    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let repo_chip = head_vs_worktree(&work);
    assert!(matches!(state.read(repo_chip.clone()), Read::Waiting));
    state.drain_events();
    let Read::Fresh(diff) = state.read(repo_chip) else {
        panic!("the worktree comparison settles");
    };
    assert!(
        body_lines(&diff.patch).any(|l| l.kind == PatchLineKind::Added && l.text == "three"),
        "HEAD vs worktree reaches the unstaged edit too:\n{}",
        diff.patch
    );

    // Switching chips is where a hand-written key comparison goes wrong: the
    // Staged target has nothing cached, and must not be answered with the Repo
    // comparison's value.
    let staged = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        DiffComparison::Staged,
        false,
        Some(PathBuf::from("README.md")),
    );
    assert!(
        matches!(state.read(staged.clone()), Read::Waiting),
        "the other comparison's value must not answer this one"
    );
    state.drain_events();
    let Read::Fresh(diff) = state.read(staged) else {
        panic!("the staged comparison has its own answer now");
    };
    assert!(
        body_lines(&diff.patch).all(|l| !l.text.contains("three")),
        "staged shows the index, not the worktree edit:\n{}",
        diff.patch
    );
}

/// The third kind: one pane's bytes, from a real repository, answered with both
/// sides resolved — and kept in the keyed map after another pane takes over,
/// which is the policy the single-entry reads above do not share.
#[test]
fn a_pane_answers_with_both_sides_and_keeps_them() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path(), false);
    let png = png_bytes();
    std::fs::write(work.join("art.png"), &png).unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "art"]);
    // A bigger second version, so the two sides differ by length as well as by
    // existence.
    std::fs::write(work.join("art.png"), [png.clone(), vec![0u8; 64]].concat()).unwrap();

    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&work));
    settle_first(&mut state);

    let comparison = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        DiffComparison::Repo,
        false,
        Some(PathBuf::from("art.png")),
    );
    let file = FileMeta {
        // Repo-relative, as the patch value answers it — the pane read no
        // longer strips a `b/` off somebody else's string.
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
    let before = entry.old.as_ref().expect("HEAD side").byte_len;
    let after = entry.new.as_ref().expect("worktree side").byte_len;
    assert_eq!(before, png.len() as u64, "the rev side came from HEAD");
    assert_eq!(after, png.len() as u64 + 64, "the worktree side grew");

    // A different pane of the same comparison is a different entry, and the
    // first is still answerable afterwards — the keep policy.
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

    let target = head_vs_worktree(&work);
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

/// The untracked preview is answered without a worker — which must not leave
/// the read holding its slot, or the shell keeps asking for frames forever and
/// the surface never settles.
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
        DiffComparison::Local,
        false,
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

/// Blame crosses the same interface with its own kind: a `Fresh` for one
/// target never answers another, and the second one waits for its own fetch.
#[test]
fn a_blame_answer_is_not_another_revisions_answer() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("blame");
    git(
        tmp.path(),
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    std::fs::write(work.join("README.md"), "one\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "first"]);
    let first = git(&work, &["rev-parse", "HEAD"]);
    std::fs::write(work.join("README.md"), "one\ntwo\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "second"]);
    let second = git(&work, &["rev-parse", "HEAD"]);

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
