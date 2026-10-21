//! Staging list & diff viewer upgrades (issue 20, screen 06) — headless
//! harness tests.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` over
//! temporary repositories, asserting painted labels and public `AppState`
//! transitions: the Commit window's flat staging file list (per-repo section
//! headers and Stage all / Unstage all removed by issue 07 — staging lives in
//! the per-file checkboxes), the diff viewer's hunk collapsing, per-hunk
//! staged state in the hunk header.

use std::path::{Path, PathBuf};

use egui_kittest::{Harness, kittest::Queryable};
use test_support::git_seed::git;
use test_support::harness::{
    assert_painted, galley_origin, painted_galleys, painted_text, shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::state::AppState;

// ---------------------------------------------------------------- helpers --

/// A changed diff row's accessibility label, spelled the way the pane spells it:
/// the row's text, then the state the per-line oracle answers for it.
fn row(text: &str, staged: bool) -> String {
    format!("{text} ({})", if staged { "staged" } else { "unstaged" })
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
///
/// Kept local, NOT `git_seed::repo_with_one_commit`: the seeded file's NAME is
/// load-bearing. `file_list_paints_flat_rows_without_section_headers_or_hunk_counts`
/// asserts on `base.txt` by name.
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

/// Headless harness over the given repository roots (see CONTEXT.md).
///
/// Kept as [`shell_harness_over_unstyled`]: the hunk-header geometry below is measured
/// against this unstyled frame, and `max_steps` is stated at 1024 because
/// `Harness::run` panics once a run exceeds kittest's 4.
fn harness(roots: &[PathBuf]) -> Harness<'static, AppState> {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let state = AppState::for_roots(&dir, roots);
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

/// Whether `needle` is among the repo's staged paths.
fn staged_contains(repo: &Repo, needle: &str) -> bool {
    git(&repo.path, &["diff", "--cached", "--name-only"]).contains(needle)
}

// ------------------------------------------- UNSTAGED / STAGED sections ----

#[test]
fn file_list_paints_flat_rows_without_section_headers_or_hunk_counts() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "sections");

    // One unstaged edit, one staged edit, one untracked file.
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "staged edit\n").unwrap();
    git(&repo.path, &["add", "other.txt"]);
    std::fs::write(repo.path.join("untracked.txt"), "new\n").unwrap();

    let h = harness(std::slice::from_ref(&repo.path));

    // The per-repo staging section headers are gone — every changed file is
    // one row of the repo's one flat, path-sorted list, untracked included.
    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("UNSTAGED") && !text.contains("STAGED"),
        "staging section headers must be removed, got:\n{text}"
    );
    for row in ["base.txt", "other.txt", "untracked.txt"] {
        h.get_by_label(row);
    }
    assert!(
        !text.contains("Unversioned Files"),
        "untracked files share the flat list; their own group is gone"
    );

    // Issue 05: hunk counts moved out of file rows into the diff header —
    // the old per-row badges ("1 hunk" galley, staged "1/1" ratio) no longer
    // paint, so each is asserted by its own galley.
    assert!(
        galley_origin(&h, "1 hunk").is_none(),
        "the unstaged row badge must be gone"
    );
    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("1/1"),
        "staged rows must not paint the staged/total ratio, got:\n{text}"
    );
}

// ------------------------------------------------- hunk collapsing ---------

/// Commit `code.rs` with two far-apart functions, then edit both — a
/// two-hunk diff whose body rows are individually addressable.
fn seed_two_hunk_edit(repo: &Repo) {
    let gap = "\n".repeat(10);
    let v1 = format!("fn alpha() {{\n    let a = 1;\n}}\n{gap}fn omega() {{\n    let o = 2;\n}}\n");
    let v2 = format!(
        "fn alpha() {{\n    let alpha = 10;\n}}\n{gap}fn omega() {{\n    let omega = 20;\n}}\n"
    );
    std::fs::write(repo.path.join("code.rs"), &v1).unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(repo.path.join("code.rs"), &v2).unwrap();
}

/// Body-row texts of the two hunks (unified addition rows).
const ALPHA_ADD: &str = "    let alpha = 10;";
const OMEGA_ADD: &str = "    let omega = 20;";

/// Open the Commit window's preview of `code.rs` and wait for the diff.
fn open_preview(h: &mut Harness<'_, AppState>) {
    h.get_by_label("code.rs").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );
    h.run();
}

#[test]
fn hunks_collapse_and_expand_on_demand() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "collapse");
    seed_two_hunk_edit(&repo);

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    // Both hunks start expanded.
    assert_painted(&h, ALPHA_ADD);
    assert_painted(&h, OMEGA_ADD);

    // Collapsing hunk 1 hides its body behind an N-hidden band…
    h.get_by_label("Collapse hunk 1").click();
    h.run();
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("hidden"),
        "the collapsed hunk must report its hidden rows, got:\n{text}"
    );
    assert!(!text.contains(ALPHA_ADD), "collapsed body rows must vanish");
    assert!(
        text.contains(OMEGA_ADD),
        "the other hunk's body must stay visible, got:\n{text}"
    );

    // …and expanding restores it.
    h.get_by_label("Expand hunk 1").click();
    h.run();
    assert_painted(&h, ALPHA_ADD);
}

// ------------------------------ a hunk header names its own staged state ----

#[test]
fn hunk_headers_name_their_own_staged_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "hunk-state");
    seed_two_hunk_edit(&repo);

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    // Stage hunk 1 through its gutter control.
    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo, "code.rs")),
        "the hunk stage must reach the index, toast={:?} busy={} staged={:?} text=\n{}",
        h.state().ui.toast,
        h.state().ui.busy,
        git(&repo.path, &["diff", "--cached", "--name-only"]),
        painted_text(&h).join("\n")
    );
    let text = painted_text(&h).join("\n");
    // The header's own staged readout is back, and it is aggregated from the
    // per-line oracle rather than from span overlap. Which hunk says what is
    // matched by row, not by count: the galleys are the painted order, so the
    // upper `@@` band is the hunk the gutter staged and the lower one is the hunk
    // nobody touched.
    let rows: Vec<(f32, String)> = {
        let mut out: Vec<(f32, String)> = painted_galleys(&h)
            .into_iter()
            .filter(|g| g.text.starts_with("@@"))
            .map(|g| (g.pos.y, g.text))
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    };
    assert_eq!(
        rows.len(),
        2,
        "the fixture's premise: two hunk headers to name, got {rows:?}"
    );
    let named_at = |y: f32| {
        painted_galleys(&h)
            .into_iter()
            .find(|g| g.text.starts_with("· ") && (g.pos.y - y).abs() < 8.0)
            .map(|g| g.text)
    };
    assert_eq!(
        named_at(rows[0].0).as_deref(),
        Some("· staged"),
        "the hunk staged through its gutter control answers every one of its rows \
         staged, got {:?}",
        named_at(rows[0].0)
    );
    assert_eq!(
        named_at(rows[1].0).as_deref(),
        Some("· not staged"),
        "and the untouched hunk answers none of them staged, got {:?}",
        named_at(rows[1].0)
    );
    // Ordered after the two asserts above on purpose. `painted_text` reads the
    // last run frame and `wait_until` never runs one, so those two are what
    // prove this frame is the post-stage one. Measured against a frame that
    // never held a staged hunk, this absence would pass with the rail still in
    // the tree.
    assert!(
        !text.contains("hunk staged") && !text.contains("hunks staged"),
        "the staged-hunk summary rail was removed deliberately, so no painted frame \
         may count staged hunks for the reader, got:\n{text}"
    );
    // A hunk header classifies staged-ness only, so no wording that names a
    // comparison axis may appear on it.
    for stale in ["· local", "· index", "· staged / local"] {
        assert!(
            !text.contains(stale),
            "a hunk header must not classify itself against a comparison that no \
             longer exists, got:\n{text}"
        );
    }
}

#[test]
fn partially_staged_hunk_names_itself_part_in_its_header() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "part-header");
    // One hunk with two separate one-line edits (within context range).
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let alpha = 1;\n    let beta = 2;\n    let gamma = 3;\n}\n",
    )
    .unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let ALPHA = 1;\n    let beta = 2;\n    let GAMMA = 3;\n}\n",
    )
    .unwrap();

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    // Two real clicks stage the whole alpha line change: its deletion row,
    // then its addition row off the refreshed preview. The gamma edit in the
    // same hunk stays unstaged, which is what makes it partial.
    h.get_by_label(&row("    let alpha = 1;", false)).click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo, "code.rs")),
        "the line stage must reach the index, toast={:?} text=\n{}",
        h.state().ui.toast,
        painted_text(&h).join("\n")
    );
    // The clicked row stays on screen, beside its pair: the combined read is
    // byte-stable under a stage, so the second click is the same row the reader
    // has been looking at all along, and its address is translated against the
    // freshly re-derived retained sections rather than a stale ordinal.
    //
    // And only the clicked row's word changed. One half of a replacement pair went
    // into the index, so it reads `staged` and its partner still reads `unstaged`,
    // which is the pair's own story rather than one the label invents.
    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&row("    let alpha = 1;", true)).is_some()
                && h.query_by_label(&row("    let ALPHA = 1;", false))
                    .is_some()
        }),
        "both rows of the staged line are still in the preview, the clicked one \
         named staged and its partner not"
    );
    h.get_by_label(&row("    let ALPHA = 1;", false)).click();
    h.run();
    assert!(
        wait_until(15_000, || {
            git(&repo.path, &["diff", "--cached", "--", "code.rs"]).contains("+    let ALPHA = 1;")
        }),
        "both clicked lines must be staged, got:\n{}",
        git(&repo.path, &["diff", "--cached", "--", "code.rs"])
    );
    // The header's `part` is the reason the text exists at all: muting cannot show
    // a mixed hunk. A hunk holding both states looks like a hunk holding some quiet
    // rows, and nobody can count those at a glance, so the rows go quiet and the
    // header says what the paint cannot.
    let mut suffixes: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("· "))
        .map(|g| g.text)
        .collect();
    suffixes.sort();
    assert_eq!(
        suffixes,
        vec!["· part".to_owned()],
        "the one hunk on screen holds both states, so it names itself part and \
         nothing else does"
    );
}
