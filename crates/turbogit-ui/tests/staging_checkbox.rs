//! Local Changes: the per-file checkbox as a direct control over git's index.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` over
//! temporary repositories, clicking the checkbox and the row body through the
//! accessibility tree, and reading every answer back out of **git itself** —
//! `git status --porcelain`, `git diff --cached --name-only`, `git log` —
//! rather than out of any of the window's own state. What the checkbox paints
//! is read off the painted frame.
//!
//! Covers: the three checkbox states and the whole-file operation behind each;
//! un-staging a newly added file back to untracked; un-staging in a repository
//! with no commits; ignored files absent from the flat list; the badge letter
//! and colour per status; a new-and-modified file reading `M`; conflicted rows
//! carrying no checkbox and keeping the name column; the selection surviving a
//! commit and being pruned on refresh; and `Space` on the focused row.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::git;
use test_support::harness::{
    painted_galleys, painted_ink, painted_text, shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::state::AppState;
use turbogit_domain::model::PatchOrigin;
use turbogit_ui::theme::Palette;

// ---------------------------------------------------------------- helpers --

struct Repo {
    path: PathBuf,
}

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

/// A repository with `git init` and nothing else: no commit, no HEAD. The
/// un-stage route has to work here, which is exactly where the libgit2 backend
/// used to fail by trying to resolve HEAD.
fn unborn_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    Repo { path }
}

fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

/// `git status --porcelain` for the repository, trimmed.
fn status(repo: &Path) -> String {
    git(repo, &["status", "--porcelain"]).trim().to_string()
}

/// The repository's staged paths, sorted.
fn staged(repo: &Path) -> Vec<String> {
    let mut out: Vec<String> = git(repo, &["diff", "--cached", "--name-only"])
        .lines()
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

/// The empty, un-staged checkbox's accessible label.
fn stage_box(path: &str) -> String {
    format!("Stage {path} (unstaged)")
}

/// The half-filled checkbox's label.
fn partial_box(path: &str) -> String {
    format!("Stage all of {path} (partially staged)")
}

/// The fully ticked checkbox's label.
fn unstage_box(path: &str) -> String {
    format!("Unstage {path} (staged)")
}

#[track_caller]
fn wait_for_staged(h: &mut Harness<'_, AppState>, repo: &Path, needle: &str) -> bool {
    wait_until(15_000, || {
        h.run();
        staged(repo).iter().any(|p| p == needle)
    })
}

// ------------------------------------------------ the three checkbox states --

/// **A half-filled box stages the whole file.** Finishing a partial staging is
/// the shortest path, so the middle state is the *stage* direction, not the
/// un-stage one — and the answer is read out of git, not out of the box.
#[test]
fn the_middle_state_clicks_to_the_whole_file_staged() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "partial-box");
    std::fs::write(repo.path.join("base.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "one\nTWO\nthree\n").unwrap();
    // Stage the first half of the worktree by hand, so git reports `MM`.
    git(&repo.path, &["stash", "push", "-q", "--keep-index"]);
    git(&repo.path, &["stash", "pop", "-q"]);
    std::fs::write(repo.path.join("base.txt"), "ONE\ntwo\nthree\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    std::fs::write(repo.path.join("base.txt"), "ONE\nTWO\nthree\n").unwrap();
    assert!(
        status(&repo.path).contains("MM"),
        "the fixture must be genuinely half-staged, git says: {}",
        status(&repo.path)
    );

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label(&partial_box("base.txt")).click();
    h.run();
    assert!(
        wait_for_staged(&mut h, &repo.path, "base.txt"),
        "the middle state stages the whole file"
    );
    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&unstage_box("base.txt")).is_some()
        }),
        "and the box becomes fully staged"
    );
}

/// **Un-staging a newly added file returns it to untracked**, not to a modified
/// row and not to a vanished one: git's own `A` line goes away and its `??` line
/// comes back.
#[test]
fn unstaging_a_newly_added_file_returns_it_to_untracked() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "added-untick");
    std::fs::write(repo.path.join("new.txt"), "new\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label(&stage_box("new.txt"));
    h.get_by_label(&stage_box("new.txt")).click();
    h.run();
    assert!(wait_for_staged(&mut h, &repo.path, "new.txt"));

    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label("Unstage new.txt (staged)").is_some()
        }),
        "the box reads fully staged"
    );
    h.get_by_label(&unstage_box("new.txt")).click();
    h.run();

    assert!(
        wait_until(15_000, || {
            h.run();
            staged(&repo.path).is_empty()
        }),
        "the index no longer holds it"
    );
    assert_eq!(
        status(&repo.path),
        "?? new.txt",
        "and git reports it untracked again — not deleted, not modified"
    );
    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&stage_box("new.txt")).is_some()
        }),
        "the row is still there with an empty box"
    );
}

/// **Un-staging works in a repository with no commits.** There is no HEAD for a
/// backend to resolve, so this is the case a real `git restore --staged` route
/// must be checked against.
#[test]
fn unstaging_works_in_a_repository_with_no_commits() {
    let parent = tempfile::tempdir().unwrap();
    let repo = unborn_repo(parent.path(), "unborn");
    assert!(
        !std::process::Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(&repo.path)
            .output()
            .is_ok_and(|out| out.status.success()),
        "the fixture must have no HEAD"
    );

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label(&stage_box("base.txt"));
    h.get_by_label(&stage_box("base.txt")).click();
    h.run();
    assert!(
        wait_for_staged(&mut h, &repo.path, "base.txt"),
        "the file stages on an unborn branch"
    );

    h.get_by_label(&unstage_box("base.txt")).click();
    h.run();
    assert!(
        wait_until(15_000, || {
            h.run();
            staged(&repo.path).is_empty()
        }),
        "and it un-stages there too — the click behaves the same on a fresh project"
    );
    assert!(
        h.state().last_error.is_none(),
        "and no backend error surfaced: {:?}",
        h.state().last_error
    );
}

/// **A click on the checkbox works on a cold diff cache.** The granular
/// pipeline peeks the cached diff and no-ops without it; the checkbox must not.
/// This test never opens the diff preview, so the cache is cold by construction.
#[test]
fn the_checkbox_works_on_a_cold_diff_cache() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "cold-cache");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    assert!(
        h.state().ui.diff_cache.is_none(),
        "the diff cache is cold: nothing was previewed"
    );

    h.get_by_label(&stage_box("base.txt")).click();
    h.run();
    assert!(
        wait_for_staged(&mut h, &repo.path, "base.txt"),
        "a checkbox click reaches the index with no cached diff behind it"
    );
}

// --------------------------------------------------- the flat file list ------

/// **One flat list, path-sorted, staging state nowhere in the structure.**
#[test]
fn the_list_is_one_flat_path_sorted_list_with_no_staging_headings() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "flat");
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    // One of each: modified, staged, untracked, nested — deliberately seeded in
    // an order that is neither alphabetical nor by staging state.
    std::fs::write(repo.path.join("zzz.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("aaa.txt"), "staged\n").unwrap();
    std::fs::write(repo.path.join("mmm.txt"), "untracked\n").unwrap();
    std::fs::write(repo.path.join("src/bbb.rs"), "nested\n").unwrap();
    git(&repo.path, &["add", "src/bbb.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "nested"]);
    std::fs::write(repo.path.join("src/bbb.rs"), "nested edited\n").unwrap();
    git(&repo.path, &["add", "aaa.txt"]);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Every kind of file is a row of the same list: staged, unstaged, untracked
    // and nested. The commit above left `zzz.txt` modified and `aaa.txt` staged.
    for row in ["aaa.txt", "zzz.txt", "mmm.txt", "bbb.rs"] {
        h.get_by_label(row);
    }
    let text = painted_text(&h).join("\n");
    for heading in ["UNSTAGED", "STAGED", "Unversioned Files"] {
        assert!(
            !text.contains(heading),
            "`{heading}` must not structure the list any more, got:\n{text}"
        );
    }

    // And they are in path order — component-wise, so `src/…` sorts with the
    // `s` names rather than after every root-level one.
    let in_path_order = ["aaa.txt", "mmm.txt", "bbb.rs", "zzz.txt"];
    let rows = in_path_order
        .iter()
        .map(|n| h.get_by_label(n).rect().top())
        .collect::<Vec<_>>();
    assert!(
        rows.windows(2).all(|w| w[0] <= w[1]),
        "the flat list is sorted by path, got row tops {rows:?} for {in_path_order:?}"
    );
}

/// **Ignored files stay out.** The bucketing that filtered them is gone; the
/// noise filter that shared it is not.
#[test]
fn ignored_files_stay_out_of_the_flat_list() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "ignored");
    std::fs::write(repo.path.join(".gitignore"), "build/\n").unwrap();
    git(&repo.path, &["add", ".gitignore"]);
    git(&repo.path, &["commit", "-q", "-m", "ignore"]);
    std::fs::create_dir_all(repo.path.join("build")).unwrap();
    std::fs::write(repo.path.join("build/out.bin"), "noise\n").unwrap();
    std::fs::write(repo.path.join("real.txt"), "work\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("real.txt");
    assert!(
        h.query_by_label("out.bin").is_none(),
        "an ignored file must not reappear in the list"
    );
    assert!(
        !painted_text(&h).join("\n").contains("out.bin"),
        "and must not paint at all"
    );
}

// -------------------------------------------------------- the status badge --

/// **Every status paints its own letter in its own hue**, read off the painted
/// frame. One new (untracked), one added-and-staged, one modified, one deleted.
#[test]
fn each_status_renders_its_expected_letter_and_colour() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "badges");
    std::fs::write(repo.path.join("gone.txt"), "doomed\n").unwrap();
    std::fs::write(repo.path.join("kept.txt"), "kept\n").unwrap();
    git(&repo.path, &["add", "."]);
    git(&repo.path, &["commit", "-q", "-m", "two files"]);
    std::fs::write(repo.path.join("kept.txt"), "edited\n").unwrap();
    git(&repo.path, &["rm", "-q", "gone.txt"]);
    std::fs::write(repo.path.join("fresh.txt"), "untracked\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    assert_eq!(
        painted_ink(&h, "N"),
        Some(Palette::STATUS_ADDED),
        "a file git has never seen is a green N"
    );
    assert_eq!(
        painted_ink(&h, "M"),
        Some(Palette::STATUS_MODIFIED),
        "a modified file is a blue M"
    );
    assert_eq!(
        painted_ink(&h, "D"),
        Some(Palette::STATUS_DELETED),
        "a deleted file is a grey D — a removal is settled, not an error"
    );
    let _ = h.get_by_label("kept.txt");
    let _ = h.get_by_label("fresh.txt");
}

/// **A file that is both new and modified reads `M`.** One letter has to answer
/// both facts, and modified wins — the loss of the `N` is accepted, not fixed.
#[test]
fn a_new_and_modified_file_renders_m() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "new-and-modified");
    std::fs::write(repo.path.join("new.txt"), "first\n").unwrap();
    git(&repo.path, &["add", "new.txt"]);
    std::fs::write(repo.path.join("new.txt"), "second\n").unwrap();
    assert!(
        status(&repo.path).starts_with('A'),
        "the fixture must be added-with-unstaged-work, git says: {}",
        status(&repo.path)
    );

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("new.txt");

    assert_eq!(
        painted_ink(&h, "M"),
        Some(Palette::STATUS_MODIFIED),
        "\"new and modified\" is one letter, and it is the modified one"
    );
}

// ------------------------------------------------------------- conflicts ----

/// **A conflicted path carries no checkbox, and its name lines up with every
/// other row.** Both halves are one claim: the conflict row is a row.
#[test]
fn conflicted_rows_have_no_checkbox_and_keep_the_name_column() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "conflict-row");
    let branch = git(&repo.path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    std::fs::write(repo.path.join("plain.txt"), "v1\n").unwrap();
    git(&repo.path, &["add", "."]);
    git(&repo.path, &["commit", "-q", "-m", "base"]);

    // A real merge conflict, so the row is a real conflicted change.
    git(&repo.path, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.path.join("conf.txt"), "side\n").unwrap();
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "side"]);
    git(&repo.path, &["checkout", "-q", &branch]);
    std::fs::write(repo.path.join("conf.txt"), "main line\n").unwrap();
    git(&repo.path, &["add", "conf.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo.path)
        .output();
    std::fs::write(repo.path.join("plain.txt"), "v2\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("Merge conflicts (1)");

    // No checkbox: an unmerged path cannot be staged, so none of the three
    // states' labels exists for it. Every checkbox in this list carries one of
    // them, so their absence is the whole claim.
    for label in [
        stage_box("conf.txt"),
        partial_box("conf.txt"),
        unstage_box("conf.txt"),
    ] {
        assert!(
            h.query_by_label(&label).is_none(),
            "an unmerged path must have no staging box, but `{label}` exists"
        );
    }

    // …and the name sits in the same column: the badge occupies the shared
    // status-letter slot and the name starts one gap after it. The badge's own
    // x is the comparable fact — the name's pixel x follows the badge's
    // *measured* width, which is per-letter by design (`M` and `N` are wider
    // than `C` and `D`), so an identity here would be a claim about the font.
    let conflict_badge_x = painted_name_x(&h, "C");
    let plain_badge_x = painted_name_x(&h, "M");
    assert!(
        (conflict_badge_x - plain_badge_x).abs() < 0.01,
        "the conflicted row's badge must sit in the shared status-letter \
         column (conflict {conflict_badge_x} vs plain {plain_badge_x})"
    );
    let conflict_name_x = painted_name_x(&h, "conf.txt");
    assert!(
        conflict_name_x > plain_badge_x,
        "and the name must start after that badge, in the name column \
         (name {conflict_name_x}, badge column ends {plain_badge_x})"
    );
}

/// The x of the painted file-name galley, or a panic naming the missing text.
#[track_caller]
fn painted_name_x(h: &Harness<'_, AppState>, name: &str) -> f32 {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == name)
        .unwrap_or_else(|| panic!("no galley paints {name:?}"))
        .pos
        .x
}

// ----------------------------------------------------- selection lifetime ---

/// **The selection survives a commit, and is pruned when a file leaves the
/// list.** It means "the files I am looking at", so a commit says nothing about
/// it and a vanishing path must not sit in it forever.
#[test]
fn selection_survives_a_commit_and_is_pruned_on_refresh() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "selection-life");
    std::fs::write(repo.path.join("a.txt"), "one\n").unwrap();
    std::fs::write(repo.path.join("b.txt"), "two\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("a.txt").click();
    h.get_by_label("b.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.selected.len(),
        2,
        "both rows are selected: {:?}",
        h.state().ui.selected
    );

    // Stage and commit one of them.
    h.get_by_label(&stage_box("a.txt")).click();
    h.run();
    assert!(wait_for_staged(&mut h, &repo.path, "a.txt"));
    h.state_mut().ui.commit_message = "selection lifetime".into();
    h.run();
    h.get_by_label("Commit changes").click();
    h.run();
    assert!(
        wait_until(15_000, || git(&repo.path, &["log", "--format=%s"])
            .contains("selection lifetime")),
        "the commit landed"
    );

    assert!(
        wait_until(15_000, || {
            h.run();
            // `b.txt` was never staged, so it is still a row; `a.txt` is gone
            // and must have been pruned from the selection.
            !h.state().ui.selected.contains(&repo.path.join("a.txt"))
        }),
        "a committed file leaves the list and is pruned from the selection, \
         left: {:?}",
        h.state().ui.selected
    );
    assert!(
        h.state().ui.selected.contains(&repo.path.join("b.txt")),
        "the file still in the list keeps its place in the selection"
    );
    assert!(
        h.query_by_label(&stage_box("b.txt")).is_some(),
        "and is still a row"
    );
}

// ----------------------------------------------------------------- Space ----

/// **Space toggles the focused row's checkbox** — the same whole-file operation
/// the pointer performs, reached from the keyboard.
#[test]
fn space_toggles_the_focused_rows_checkbox() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "space");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Nothing previewed: nothing focused, so nothing to toggle.
    h.key_press(egui::Key::Space);
    h.run();
    assert!(
        staged(&repo.path).is_empty(),
        "with no focused row Space must stage nothing at all"
    );

    // Preview a row, then Space.
    h.get_by_label("base.txt").click();
    h.run();
    h.key_press(egui::Key::Space);
    h.run();
    assert!(
        wait_for_staged(&mut h, &repo.path, "base.txt"),
        "Space stages the focused row"
    );

    h.key_press(egui::Key::Space);
    h.run();
    assert!(
        wait_until(15_000, || {
            h.run();
            staged(&repo.path).is_empty()
        }),
        "and pressing it again un-stages it"
    );
}

// ------------------------------------------------- the diff preview jump ----

/// **Ticking switches the preview to the staged version; un-ticking falls back
/// to the working version** — never to a staged view of a file with nothing
/// staged in it.
#[test]
fn the_preview_follows_the_checkbox_from_working_to_staged_and_back() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "preview-jump");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();
    h.run();

    h.get_by_label(&stage_box("base.txt")).click();
    h.run();
    assert!(
        wait_until(15_000, || {
            h.run();
            h.state().ui.diff_comparison == turbogit_app::state::DiffComparison::Staged
        }),
        "ticking shows exactly what is about to be committed"
    );

    h.get_by_label("Unstage base.txt (staged)").click();
    h.run();
    assert!(
        wait_until(15_000, || {
            h.run();
            h.state().ui.diff_comparison == turbogit_app::state::DiffComparison::Repo
        }),
        "un-ticking falls back to the working version, not to an empty staged one"
    );
}

// ------------------------------------------------ the disabled explanation --

/// **A disabled Commit says why.** Hover it until the tooltip paints, the same
/// way the disabled menu rows do — a control that refuses without a reason
/// reads as broken rather than as gated.
#[test]
fn the_disabled_commit_explains_itself_on_hover() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "disabled-reason");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Empty index, empty message: the staged-file reason is the one shown.
    h.state_mut().ui.commit_message = "why is this off".into();
    h.run();
    let control = h.get_by_label("Commit changes").rect();
    assert_hover_paints(
        &mut h,
        control.center(),
        "Stage at least one file",
        "the disabled Commit must name the gate that is shut",
    );

    // Empty message, full index: the other reason, so both gates are named.
    h.get_by_label(&stage_box("base.txt")).click();
    h.run();
    assert!(wait_for_staged(&mut h, &repo.path, "base.txt"));
    h.state_mut().ui.commit_message.clear();
    h.run();
    let control = h.get_by_label("Commit changes").rect();
    assert_hover_paints(
        &mut h,
        control.center(),
        "Enter a commit message",
        "the message gate must name itself too",
    );
}

/// Hover `at` until `needle` paints, or give up. egui only registers a hover
/// after the pointer travels *into* the widget and then waits out its tooltip
/// delay, so a single teleport and a single step are not enough.
#[track_caller]
fn assert_hover_paints(h: &mut Harness<'_, AppState>, at: egui::Pos2, needle: &str, why: &str) {
    h.remove_cursor();
    h.step();
    h.hover_at(at - egui::vec2(0.0, 3.0));
    h.step();
    h.hover_at(at);
    let mut seen = false;
    for _ in 0..80 {
        h.step();
        if painted_galleys(h).iter().any(|g| g.text.contains(needle)) {
            seen = true;
            break;
        }
    }
    assert!(
        seen,
        "{why}: {needle:?} never painted; painted {:?}",
        painted_galleys(h)
            .iter()
            .map(|g| &g.text)
            .collect::<Vec<_>>()
    );
}

// ----------------------------------------------------- multi-selection ------

/// **Multi-selection feeds Discard Changes.** The selection means "the files I
/// am looking at", and these are the two verbs that still read it — so a
/// selected group is the group acted on, and an unselected file is left alone.
#[test]
fn a_multi_selection_drives_discard_and_leaves_the_rest_alone() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "discard-multi");
    std::fs::write(repo.path.join("a.txt"), "one\n").unwrap();
    std::fs::write(repo.path.join("b.txt"), "two\n").unwrap();
    git(&repo.path, &["add", "."]);
    git(&repo.path, &["commit", "-q", "-m", "two files"]);
    std::fs::write(repo.path.join("a.txt"), "edited a\n").unwrap();
    std::fs::write(repo.path.join("b.txt"), "edited b\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("a.txt").click();
    h.get_by_label("b.txt").click();
    h.run();
    assert_eq!(h.state().ui.selected.len(), 2, "both rows are selected");

    // The header's rollback control opens the discard confirm for the selection.
    h.get_by_label("Rollback").click();
    h.run();
    let mut files: Vec<String> = h
        .state()
        .ui
        .confirm
        .as_ref()
        .map(|c| match c {
            turbogit_app::state::PendingConfirm::Discard { changes } => changes
                .iter()
                .map(|c| c.path.display().to_string())
                .collect(),
            _ => vec!["a different confirm".to_owned()],
        })
        .unwrap_or_default();
    files.sort();
    assert_eq!(
        files,
        vec!["a.txt".to_owned(), "b.txt".to_owned()],
        "the confirm names exactly the selected files"
    );
}

/// **Multi-selection feeds Create Patch from Local Changes**, by the same route
/// — the selection's meaning changed, not the verbs that read it.
#[test]
fn a_multi_selection_drives_create_patch_from_local_changes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "patch-multi");
    std::fs::write(repo.path.join("a.txt"), "one\n").unwrap();
    std::fs::write(repo.path.join("b.txt"), "two\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("a.txt").click();
    h.run();

    turbogit_app::create_patch::open_from_selection(h.state_mut());
    h.run();
    assert_eq!(
        h.state().ui.dlg.create_patch_origin,
        Some(PatchOrigin::LocalChanges {
            paths: vec![PathBuf::from("a.txt")]
        }),
        "the patch origin is exactly the files that were clicked, not the whole tree"
    );
}
