//! The unborn preview's failure presentation, painted through the real shell.
//!
//! Issue 08's product rule: when the root-wide read of the view a pane
//! displays has failed, the whole file counts as failed to read. The pane
//! paints the engine's sentence and nothing else, because a rendered diff
//! whose every row click the composition path declines by name is a control
//! painted available and silently inert. Every fixture is a real temporary
//! repository, the harness drives `turbogit_ui::ui::render` headlessly, and
//! the answer about the index is read back out of `git status --porcelain`
//! and `git diff --cached`, not out of a flag.

use std::path::{Path, PathBuf};

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{git, pin_eol_lf};
use test_support::harness::{assert_not_painted, assert_painted, painted_galleys, painted_text};
use turbogit_app::state::AppState;

const NO_COMMITS_MESSAGE: &str =
    "this repository has no commits yet: there is no first commit to compare your changes against";

/// A repository with `git init`, one tracked edit, and no commit at all: the
/// natural root-wide `HEAD` failure, refused by `guard_unborn_head`.
fn unborn_tracked_repo(parent: &Path, name: &str) -> PathBuf {
    let work = parent.join(name);
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    pin_eol_lf(&work);
    std::fs::write(work.join("base.txt"), "alpha\nbravo\n").unwrap();
    git(&work, &["add", "--", "base.txt"]);
    work
}

fn harness(work: &Path, parent: &Path) -> Harness<'static, AppState> {
    let state = AppState::for_roots(parent, std::slice::from_ref(&work.to_path_buf()));
    let mut h = test_support::harness::shell_harness_over(state, Vec2::new(1024.0, 900.0));
    settle(&mut h);
    h
}

/// Step frames until the painted output and the keyed reads' activity
/// stabilize, so an in-flight `DiffReady` cannot be mistaken for a settled
/// frame (the same settle the diff viewer suite derives).
fn settle(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|read={}",
            painted_text(harness),
            harness.state().read_pending()
        );
        if fingerprint == prev && !harness.state().read_pending() {
            return;
        }
        prev = fingerprint;
    }
    panic!("the shell did not settle within 15s");
}

/// Open the commit tab's inline preview for one file and let the pane answer.
fn open_preview(harness: &mut Harness<'_, AppState>, file: &Path) {
    harness.state_mut().ui.preview_change = Some(file.to_path_buf());
    settle(harness);
}

/// **The operator's rule, painted.** An unborn repository has no answer to
/// the pinned `HEAD ↔ worktree` read, so the previewed file is failed to
/// read in full: the pane shows the sentence, paints no content rows, and
/// paints no staged-ness claim of any kind.
#[test]
fn an_unborn_preview_paints_the_sentence_and_no_rows_and_no_staged_ness_label() {
    let parent = tempfile::tempdir().unwrap();
    let work = unborn_tracked_repo(parent.path(), "unborn-painted");
    let mut h = harness(&work, parent.path());
    open_preview(&mut h, &work.join("base.txt"));

    assert_painted(&h, NO_COMMITS_MESSAGE);
    // The file's content is not on screen, and nothing that wears the row's
    // accessibility wording exists: a row label is always `<line text>
    // (<state>)`, so a label containing the line text is the row, painted and
    // clickable. The changelist's whole-file checkbox says `(staged)` about
    // the file and stays; the rule is about the pane's rows.
    for line in ["alpha", "bravo"] {
        assert_not_painted(&h, line);
        assert!(
            h.query_all_by_label_contains(line).next().is_none(),
            "no changed row of the unreadable file may be painted or clickable"
        );
    }
    assert!(
        h.query_all_by_label_contains("(staged-ness unknown)")
            .next()
            .is_none(),
        "a file with no painted rows has no unanswered row either"
    );
    let headers: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("@@ ") || g.text.starts_with("· "))
        .map(|g| g.text)
        .collect();
    assert!(
        headers.is_empty(),
        "and no hunk header or staged-ness suffix survives the refusal: {headers:?}"
    );
}

/// **A click cannot reach the unreadable file, and git says so.** The pane
/// paints nothing clickable for the refused preview, so the granular route
/// has no entry to compose from; the evidence is git's own index, byte for
/// byte the same after the painted frames as before them.
#[test]
fn painting_the_unreadable_preview_starts_no_git_operation() {
    let parent = tempfile::tempdir().unwrap();
    let work = unborn_tracked_repo(parent.path(), "unborn-noop");
    let status_before = git(&work, &["status", "--porcelain"]);
    let staged_before = git(&work, &["diff", "--cached"]);

    let mut h = harness(&work, parent.path());
    open_preview(&mut h, &work.join("base.txt"));
    assert_painted(&h, NO_COMMITS_MESSAGE);
    for _ in 0..5 {
        h.run();
    }

    assert_eq!(
        git(&work, &["status", "--porcelain"]),
        status_before,
        "rendering a failed preview is not an operation"
    );
    assert_eq!(
        git(&work, &["diff", "--cached"]),
        staged_before,
        "and the index is exactly where the fixture left it: nothing was \
         staged or unstaged behind the refusal"
    );
    // No row button exists to click: the only labelled buttons the pane
    // paints for rows carry the row's text, and the text is absent.
    assert!(
        h.query_all_by_label_contains("alpha").next().is_none(),
        "the refusal leaves no clickable changed row"
    );
}

/// The exemption, painted: an unversioned file in the same unborn repository
/// is *not* marked unreadable. Its pane never asked git the question that
/// failed (the answer is synthesized from the file's bytes), and its staging
/// route composes from that same value, so its rows stay on screen and stay
/// live. This pins the boundary of the operator's rule against overreach.
#[test]
fn an_unborn_preview_of_an_unversioned_file_still_paints_its_rows() {
    let parent = tempfile::tempdir().unwrap();
    let work = parent.path().join("unborn-untracked");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    pin_eol_lf(&work);
    std::fs::write(work.join("scratch.txt"), "untracked line\n").unwrap();

    let mut h = harness(&work, parent.path());
    open_preview(&mut h, &work.join("scratch.txt"));

    assert!(
        !painted_text(&h)
            .iter()
            .any(|t| t.contains("no commits yet")),
        "the synthesized untracked pane is exempt from the refusal"
    );
    let rows: Vec<String> = h
        .query_all_by_label_contains("(staged-ness unknown)")
        .map(|_| "row".to_owned())
        .collect();
    assert!(
        !rows.is_empty(),
        "its changed row is painted and labelled, exactly as ticket 04b left it"
    );
    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t.contains("untracked line")),
        "and the content the rule does not reach stays visible"
    );
}
