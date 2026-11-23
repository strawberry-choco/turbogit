use std::path::{Path, PathBuf};

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{git, pin_eol_lf};
use test_support::harness::{assert_not_painted, assert_painted, painted_galleys, painted_text};
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, RootId};

const NO_COMMITS_MESSAGE: &str =
    "this repository has no commits yet: there is no first commit to compare your changes against";

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

fn open_preview(harness: &mut Harness<'_, AppState>, root: &Path, file: &Path) {
    harness.state_mut().ui.preview_change = Some(ChangeRef::new(
        RootId(root.to_path_buf().into()),
        file.to_path_buf(),
    ));
    settle(harness);
}

#[test]
fn an_unborn_preview_paints_the_sentence_and_no_rows_and_no_staged_ness_label() {
    let parent = tempfile::tempdir().unwrap();
    let work = unborn_tracked_repo(parent.path(), "unborn-painted");
    let mut h = harness(&work, parent.path());
    open_preview(&mut h, &work, &work.join("base.txt"));

    assert_painted(&h, NO_COMMITS_MESSAGE);
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

#[test]
fn painting_the_unreadable_preview_starts_no_git_operation() {
    let parent = tempfile::tempdir().unwrap();
    let work = unborn_tracked_repo(parent.path(), "unborn-noop");
    let status_before = git(&work, &["status", "--porcelain"]);
    let staged_before = git(&work, &["diff", "--cached"]);

    let mut h = harness(&work, parent.path());
    open_preview(&mut h, &work, &work.join("base.txt"));
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
    assert!(
        h.query_all_by_label_contains("alpha").next().is_none(),
        "the refusal leaves no clickable changed row"
    );
}

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
    open_preview(&mut h, &work, &work.join("scratch.txt"));

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
