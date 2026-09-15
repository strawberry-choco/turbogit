//! Staging list & diff viewer upgrades (issue 20, screen 06) — headless
//! harness tests.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` over
//! temporary repositories, asserting painted labels and public `AppState`
//! transitions: the Commit window's flat staging file list (per-repo section
//! headers and Stage all / Unstage all removed by issue 07 — staging lives in
//! the per-file checkboxes), the diff viewer's hunk collapsing, per-hunk
//! staged state in the hunk header, and the staged-hunk chip rail.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui_kittest::{Harness, kittest::Queryable};
use test_support::harness::{assert_painted, galley_origin, painted_text};
use turbogit_app::state::AppState;

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

/// Headless harness over the given repository roots (see CONTEXT.md).
fn harness(roots: &[PathBuf]) -> Harness<'static, AppState> {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let state = AppState::for_roots(&dir, roots);
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(1280.0, 800.0));
    test_support::harness::settle(&mut h);
    h
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

/// Whether `needle` is among the repo's staged paths.
fn staged_contains(repo: &Repo, needle: &str) -> bool {
    git(&repo.path, &["diff", "--cached", "--name-only"]).contains(needle)
}

/// Poll until `needle` is painted, running frames while waiting (async ops
/// complete only through drained frames).
fn wait_until_painted(h: &mut Harness<'_, AppState>, ms: u64, needle: &str) -> bool {
    wait_until(ms, || {
        h.run();
        painted_text(h).join("\n").contains(needle)
    })
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

    // Issue 07: the per-repo staging section headers are gone — rows paint
    // flat under the repo group (issue 20's hunk-rail still counts staged
    // hunks separately). Untracked files live in the bottom group (issue 04).
    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("UNSTAGED") && !text.contains("STAGED"),
        "staging section headers must be removed, got:\n{text}"
    );
    for row in ["base.txt", "other.txt", "untracked.txt"] {
        h.get_by_label(row);
    }
    h.get_by_label("Unversioned Files (1)");

    // Issue 05: hunk counts moved out of file rows into the diff header —
    // the old per-row badges ("1 hunk" galley, staged "1/1" ratio) no longer
    // paint. (The staged-hunks rail still legitimately paints "1 hunk
    // staged" for other.txt, so assert on the exact badge galleys.)
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

// -------------------------------- staged state + staged-hunk chip rail -----

#[test]
fn hunk_headers_show_staged_state_and_rail_counts_staged_hunks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "hunk-state");
    seed_two_hunk_edit(&repo);

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    // The staged-state annotation lives on the Repo comparison (HEAD↔worktree),
    // where staged and unstaged hunks coexist.
    h.get_by_label("Repo").click();
    h.run();
    assert!(
        painted_text(&h).join("\n").contains("not staged"),
        "both hunks start unstaged"
    );

    // Stage hunk 1 through its gutter control.
    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo, "code.rs")),
        "the hunk stage must reach the index, comparison={:?} toast={:?} busy={} staged={:?} text=\n{}",
        h.state().ui.diff_comparison,
        h.state().ui.toast,
        h.state().ui.busy,
        git(&repo.path, &["diff", "--cached", "--name-only"]),
        painted_text(&h).join("\n")
    );
    assert!(
        wait_until_painted(&mut h, 15_000, "1 hunk staged"),
        "the rail must count the staged hunk"
    );

    // Back on the Repo comparison, the staged hunk's header says so and the
    // unstaged one still reads "not staged".
    h.get_by_label("Repo").click();
    h.run();
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("staged") && text.contains("not staged"),
        "hunk headers must show their per-hunk staged state, got:\n{text}"
    );
    assert!(
        text.contains("code.rs @@1") && !text.contains("(part)"),
        "the rail must chip the fully staged hunk without a part marker, got:\n{text}"
    );
}

#[test]
fn partially_staged_hunk_gets_a_part_chip() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "part-chip");
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

    // Select one changed line (Line granularity is the default) and stage it.
    h.get_by_label("    let alpha = 1;").click();
    h.run();
    eprintln!("selections={:?}", h.state().ui.line_selections);
    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo, "code.rs")),
        "the line stage must reach the index, selections={:?} comparison={:?} toast={:?} text=\n{}",
        h.state().ui.line_selections,
        h.state().ui.diff_comparison,
        h.state().ui.toast,
        painted_text(&h).join("\n")
    );
    assert!(
        wait_until_painted(&mut h, 15_000, "1 hunk staged"),
        "the rail must count the staged hunk"
    );
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("code.rs @@1 (part)"),
        "a hunk with an unstaged remainder must carry the part marker, got:\n{text}"
    );
}
