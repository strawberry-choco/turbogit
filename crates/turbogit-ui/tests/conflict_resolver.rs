//! Issue #22 — Editable conflict resolver (screen 07) tests.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] against a
//! temporary repository seeded with a REAL merge conflict, asserting only on
//! public surfaces:
//!
//! - **Painted output** — three panes, header actions, file list groups,
//!   per-conflict action labels, "Edit manually", "Auto-advance" toggle,
//!   "Apply to all N remaining".
//! - **State transitions** — public `AppState.ui.conflict_*` fields and the
//!   on-disk file after Apply.
//!
//! Covered behavior (matches screen 07 + issue acceptance criteria):
//! - the redesigned resolver is a dedicated surface (separate from the
//!   existing inline 3-way merge editor), reachable when any conflict exists
//! - three EQUAL panes Local | Result | Incoming with discrete conflict
//!   blocks, free-text editing of the Result pane replaces the composed text
//! - per-conflict actions: Take ours / Take theirs / Take both (theirs first)
//! - per-conflict Undo restores the conflict to unresolved
//! - Prev/Next navigate across the file's conflicts, with Auto-advance
//!   moving focus after each resolution
//! - "Apply to all N remaining" applies the current choice to every
//!   still-unresolved conflict in the file
//! - the left list splits auto-merged (resolved by git without interaction)
//!   from conflicted files
//! - header Abort merge restores pre-merge state; Continue merge completes
//!   the merge once every conflicted file is resolved

use std::path::{Path, PathBuf};
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, shell_harness_over_unstyled,
};

use egui::{Color32, Pos2, Rect, Shape, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use turbogit_app::state::AppState;

use test_support::git_seed::git;
use test_support::srcscan;
use test_support::wait_until;
use turbogit_ui::theme::Palette;
// ---------------------------------------------------------------- helpers --

/// The resolver module's own source, read at compile time.
const RESOLVER_SRC: &str = include_str!("../src/ui/conflict_resolver.rs");

/// Composed text for resolutions [ours, theirs] on the seeded two-conflict
/// file. Spelled out here, in the resolver's own suite, because this is where
/// the cross-surface comparison lives: the same two choices must land the
/// same bytes whichever surface took them.
const PARITY_COMPOSED: &str =
    "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";

/// Run `git` WITHOUT asserting success, and deliberately NOT
/// `test_support::git_seed::git`: the seeds below end in a `git merge` expected
/// to exit non-zero, and that refusal is what leaves the conflict state.
fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly.
/// Local builder, not a `git_seed` recipe: `init -q` with **no `-b main`**, so
/// the default branch is the machine's, and the base commit is `base.txt`.
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

impl Repo {
    fn branch(&self) -> String {
        git(&self.path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_string()
    }
}

/// Seed `conf.txt` with TWO well-separated conflict hunks on `repo`'s
/// default branch: ours rewrites lines 1 and 9 to `MAIN-*`, theirs to
/// `SIDE-*`. The seven unchanged lines between the edits keep git from
/// coalescing both hunks into a single conflict region.
fn seed_two_conflicts(repo: &Path, branch: &str) {
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "base conf"]);
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected to conflict
}

/// Seed a merge where `auto.txt` is auto-merged cleanly while `conf.txt`
/// is conflicted. Both files diverge on both branches but only `auto.txt`
/// has IDENTICAL changes on both sides so git resolves it itself.
fn seed_merge_with_auto_merged(repo: &Path, branch: &str) {
    // base: auto.txt = "shared-0\n", conf.txt = ten lines
    std::fs::write(repo.join("auto.txt"), "shared-0\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);

    // side: auto.txt to "shared-1\n", conf.txt to SIDE-*
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("auto.txt"), "shared-1\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "side"]);
    git(repo, &["checkout", "-q", branch]);

    // main: auto.txt to "shared-1\n" (identical to side), conf.txt to MAIN-*
    std::fs::write(repo.join("auto.txt"), "shared-1\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "main"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected: auto clean, conf conflict
}

/// Seed a merge with TWO different file-level clash kinds: `conf.txt` is
/// BOTH MODIFIED (UU: both branches rewrote it) and `victim.txt` is DELETED
/// BY THEM (UD: ours rewrote it, theirs deleted it — a modify/delete clash).
/// The resolver's file list must name each kind on its row so the merge can
/// be triaged by reading the list.
fn seed_two_conflict_kinds(repo: &Path, branch: &str) {
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    std::fs::write(repo.join("victim.txt"), "kept by ours\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base files"]);
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(repo, &["rm", "-q", "victim.txt"]); // theirs deletes
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    // Ours REWRITES victim.txt — an untouched file's deletion merges cleanly
    // and no UD row would exist to name.
    std::fs::write(repo.join("victim.txt"), "rewritten by ours\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected: UU + UD
}

/// Headless harness over the given repository roots.
fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

/// Headless harness driving the full app UI with event draining per frame.
///
/// `4096` is required, not tuned: this suite calls `Harness::run()` eighteen
/// times and `run()` is the path that enforces the step budget, which kittest
/// defaults to 4 — the styled constructor takes no budget. Unstyled also means
/// no `configure_style`/`install_fonts`, and the pane widths and marker tints
/// here are measured against egui's default face.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 4096)
}

// --------------------------------------------- tint + geometry mirror fns --

fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    let bg = Palette::BG;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), accent.r()),
        mix(bg.g(), accent.g()),
        mix(bg.b(), accent.b()),
    )
}

fn yours_bg() -> Color32 {
    tint_over_bg(Palette::STATE_INFO, 0.12)
}

fn theirs_bg() -> Color32 {
    tint_over_bg(Palette::STATE_ERROR, 0.12)
}

fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, 0.15)
}
/// Stroked rectangles the last frame painted, as `(rect, stroke colour)`.
///
/// Deliberately NOT `test_support::harness::stroked_rects`, a *different* query:
/// it returns a three-tuple carrying the width and filters on `width > 0.0`, so
/// a zero-width stroke passes here and is excluded there.
fn stroked_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) if rs.stroke != egui::Stroke::NONE => Some((rs.rect, rs.stroke.color)),
            _ => None,
        })
        .collect()
}

/// Paint-time origin of the first text galley whose content equals `text`
/// modulo trailing newlines.
fn origin_of(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text().trim_end() == text => Some(shape.pos),
            _ => None,
        })
}

/// The pane header's own hairline across `label`'s column: the shared pane
/// header's one `RULE_STRUCTURAL` rule, the nearest one below the label.
///
/// The header used to paint a `SURFACE`-filled box around its label; it no
/// longer does, because a pane header is a band and a hairline and a stroke
/// would mean the header floats. The assertion downstream is unchanged in
/// meaning: the three panes are the same width.
fn header_band(harness: &Harness<'_, AppState>, label: &str) -> Rect {
    let pos = galley_origin(harness, label)
        .unwrap_or_else(|| panic!("{label} header label was not painted"));
    let rules: Vec<Rect> = filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::RULE_STRUCTURAL && r.left() <= pos.x && pos.x <= r.right())
        .map(|(r, _)| r)
        .collect();
    rules
        .iter()
        .copied()
        .min_by_key(|r| ((r.top() - pos.y).abs() * 10.0) as i64)
        .unwrap_or_else(|| {
            panic!(
                "{label} header band was not painted: the label is at {pos:?} and the \
                 structural rules spanning it are {rules:?}"
            )
        })
}

/// Open the redesigned conflict resolver for the seeded conflicted file
/// via the Commit tool window's new "Resolve…" entry point.
///
/// The Commit tab's conflict section hosts one `Resolve…` PER conflicted
/// file, so a two-kind seed makes the label ambiguous; the FIRST entry opens
/// the resolver on the first conflicted file, which is every suite here
/// wants.
fn open_resolver(h: &mut Harness<'_, AppState>) {
    h.get_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().is_some_and(|l| l == "Resolve…"))
        .expect("a Resolve… entry point must be reachable")
        .click();
    h.run();
}

// ------------------------------------------------------------------ tests --

/// RED: The redesigned resolver surface is reachable while any conflict
/// exists. It must render the three pane headers AND the new header
/// actions (Abort merge / Continue merge / Auto-advance / Apply to all).
#[test]
fn redesigned_resolver_renders_three_panes_and_header_actions() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "resolver-panes-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Three pane headers paint.
    assert_painted(&h, "Local (Yours)");
    assert_painted(&h, "Result");
    assert_painted(&h, "Incoming (Theirs)");

    // Header actions visible while a merge is in progress.
    assert_painted(&h, "Abort merge");
    assert_painted(&h, "Continue merge");

    // Per-conflict and global action labels visible.
    assert_painted(&h, "Take ours");
    assert_painted(&h, "Take theirs");
    assert_painted(&h, "Take both"); // theirs first per spec
    assert_painted(&h, "Edit manually");
    assert_painted(&h, "Apply to all");
    assert_painted(&h, "Auto-advance to next conflict");
}

/// The active Result cell is a real multiline TextEdit exposed through
/// AccessKit; typing through that widget must reach the resolver's public
/// editing state.
#[test]
fn result_pane_exposes_multiline_editor_and_accepts_text_input() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "edit-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    let editor = h
        .query_all_by_role(Role::MultilineTextInput)
        .find(|node| node.value().is_some_and(|value| value.contains("MAIN-one")))
        .expect("the active Result cell must expose its composed value");
    editor.focus();
    h.run();
    let focused_editor = h
        .query_all_by_role(Role::MultilineTextInput)
        .find(|node| node.value().is_some_and(|value| value.contains("MAIN-one")))
        .expect("the active Result editor must remain in the accessibility tree");
    assert!(
        focused_editor.is_focused(),
        "the multiline Result editor must accept keyboard focus"
    );

    focused_editor.type_text("CUSTOM-RESULT\n");
    h.run();

    assert!(h.state().ui.conflict_resolver_edited);
    assert!(
        h.state().ui.conflict_text.contains("CUSTOM-RESULT"),
        "text typed into the Result TextEdit must update the resolver buffer: {:?}",
        h.state().ui.conflict_text
    );
}

/// RED: "Take both · theirs first" resolves a conflict by keeping both
/// sides with THEIRS before OURS (the spec order; existing "Ignore"
/// choice is ours before theirs).
#[test]
fn take_both_theirs_first_keeps_both_with_theirs_before_ours() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "both-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);
    assert_eq!(
        h.get_by_label("Take both 1").accesskit_node().role(),
        Role::Button,
        "resolver choices must remain accessible buttons"
    );

    h.get_by_label("Take both 1").click();
    h.run();

    let text = &h.state().ui.conflict_text;
    let theirs_pos = text.find("SIDE-one").expect("theirs in composed text");
    let ours_pos = text.find("MAIN-one").expect("ours in composed text");
    assert!(
        theirs_pos < ours_pos,
        "Take both must place theirs BEFORE ours, got:\n{text}"
    );
}

/// RED: Per-conflict Undo restores the conflict to unresolved and recomposes.
#[test]
fn per_conflict_undo_reverts_resolution() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "undo-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    h.get_by_label("Take ours 1").click();
    h.run();
    assert_eq!(h.state().ui.conflict_res[0], Some(0));
    assert_painted(&h, "1 conflict remaining");

    h.get_by_label("Undo 1").click();
    h.run();
    assert_eq!(
        h.state().ui.conflict_res[0],
        None,
        "Undo must revert the conflict to unresolved"
    );
    assert_painted(&h, "2 conflicts remaining");
}

/// RED: Prev/Next navigate between the file's conflicts; the active one
/// is the one whose actions appear in the action row.
#[test]
fn prev_next_navigates_between_conflicts() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "nav-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Initially on conflict 1.
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 0);

    h.get_by_label("Next >").click();
    h.run();
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 1);

    h.get_by_label("< Prev").click();
    h.run();
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 0);
}

/// RED: With Auto-advance enabled, resolving the current conflict moves
/// focus to the next unresolved conflict in the file.
#[test]
fn auto_advance_moves_focus_after_resolution() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "autoadv-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Toggle auto-advance on.
    h.get_by_label("Auto-advance to next conflict").click();
    h.run();
    assert!(h.state().ui.conflict_resolver_auto_advance);

    // Resolving the active conflict advances the cursor.
    h.get_by_label("Take ours 1").click();
    h.run();
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 1);
}

/// RED: "Apply to all N remaining" applies the active conflict's choice
/// to every still-unresolved conflict in the file.
#[test]
fn apply_to_all_remaining_resolves_every_conflict_in_the_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "applyall-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Resolve the first conflict via the active buttons so a recorded
    // choice exists; "Apply to all N remaining" applies that choice to
    // every other still-unresolved conflict in the file.
    h.get_by_label("Take theirs 1").click();
    h.run();
    h.get_by_label("Apply to all 1 remaining").click();
    h.run();

    assert!(
        h.state().ui.conflict_res.iter().all(|r| r.is_some()),
        "Apply to all must resolve every conflict in the file, got {:?}",
        h.state().ui.conflict_res
    );
}

/// RED: The left list splits conflicted files from auto-merged files.
#[test]
fn left_list_splits_auto_merged_from_conflicted() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "split-repo");
    let branch = repo.branch();
    seed_merge_with_auto_merged(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    assert_painted(&h, "CONFLICTED FILES");
    assert_painted(&h, "AUTO-MERGED");
    assert_painted(&h, "conf.txt");
    assert_painted(&h, "auto.txt");
}

/// A merge with one `both modified` and one
/// `deleted by them` file paints both status words on their own rows.
/// Scoped by each row's `Node::rect()`: "modified"/"deleted" also appear in
/// the change-kind colouring elsewhere in the Commit window behind the
/// resolver, so an unscoped substring assert would match the wrong row.
#[test]
fn file_list_rows_paint_their_file_level_conflict_kind() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "kinds-repo");
    let branch = repo.branch();
    seed_two_conflict_kinds(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    for (path, kind_word) in [
        ("conf.txt", "both modified"),
        ("victim.txt", "deleted by them"),
    ] {
        let rect = h
            .get_all_by_role(Role::Button)
            .find(|n| {
                n.accesskit_node()
                    .label()
                    .is_some_and(|label| label.contains(kind_word))
            })
            .unwrap_or_else(|| panic!("no file-list row carries the {kind_word} kind for {path}"))
            .rect();
        assert!(
            painted_galleys(&h)
                .iter()
                .any(|g| g.text.contains(kind_word) && rect.contains(g.pos)),
            "`{kind_word}` must paint inside the {path} row at {rect:?}"
        );
    }
}

/// Negative case: a bulk apply with nothing left to
/// resolve reports the NEGATIVE result instead of staying silent — the
/// outcome that regresses silently.
#[test]
fn bulk_apply_reports_the_negative_result_when_nothing_resolves() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "bulk-negative-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Resolve both conflicts individually, so the bulk apply has nothing
    // left to do — then click it and expect it to say so. The second take
    // lands on a file that already carries merged changes, so the Overwrite
    // Changes pre-check holds it and the dangerous affirmative
    // is what applies it.
    h.get_by_label("Take ours 1").click();
    h.run();
    h.get_by_label("Next >").click();
    h.run();
    h.get_by_label("Take ours 2").click();
    h.run();
    h.get_by_label("Discard and Accept").click();
    h.run();

    // The control stays clickable at zero remaining — that is how the
    // negative result is reportable at all.
    h.get_by_label("Apply to all 0 remaining").click();
    h.run();

    assert_painted(&h, "No conflicts were resolved automatically");
    assert_not_painted(&h, "All conflicts were resolved automatically");
}

/// Positive case: a bulk apply that resolves
/// everything reports the positive result — a distinct string for a
/// distinct outcome.
#[test]
fn bulk_apply_reports_all_resolved_when_everything_resolves() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "bulk-positive-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    h.get_by_label("Apply to all 2 remaining").click();
    h.run();

    assert_painted(&h, "All conflicts were resolved automatically");
    assert_not_painted(&h, "No conflicts were resolved automatically");
}

/// The finish control's fill is WITHHELD — no
/// fill rect for its slot, not a tinted one — while anything is unresolved,
/// and paints `Palette::ACCENT` once everything is resolved. Assert the
/// rect's absence/presence at the control's own slot, never a colour change:
/// a disabled control in this design is withheld, not greyed.
#[test]
fn finish_control_fill_is_withheld_until_every_conflict_is_resolved() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "finish-fill-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    let accent_fill_on_finish = |h: &Harness<'_, AppState>| {
        let slot = h.get_by_label("Apply").rect();
        filled_rects(h)
            .iter()
            .any(|(r, c)| *c == Palette::ACCENT && r.contains(slot.center()))
    };

    // None resolved: withheld — no fill paints for the finish slot.
    assert!(
        !accent_fill_on_finish(&h),
        "the finish control must paint no fill while nothing is resolved"
    );

    // Partially resolved: still withheld.
    h.get_by_label("Take ours 1").click();
    h.run();
    assert!(
        !accent_fill_on_finish(&h),
        "the finish control must paint no fill while conflicts remain"
    );

    // All resolved: the finish control paints its ACCENT fill. The second
    // take is held by the Overwrite Changes pre-check; the
    // dangerous affirmative applies it.
    h.get_by_label("Next >").click();
    h.run();
    h.get_by_label("Take ours 2").click();
    h.run();
    h.get_by_label("Discard and Accept").click();
    h.run();
    assert!(
        accent_fill_on_finish(&h),
        "the finish control must paint its ACCENT fill once every conflict is resolved"
    );
}

/// RED: Abort merge restores the pre-merge state — the conflicted file
/// disappears and the index is no longer mid-merge.
#[test]
fn abort_merge_restores_pre_merge_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "abort-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    h.get_by_label("Abort merge").click();
    h.run();

    assert!(
        wait_until(10_000, || {
            // After `git merge --abort`, the unmerged paths are gone and
            // HEAD points back at the pre-merge commit.
            let status = git(&repo.path, &["status", "--porcelain"]);
            !status.lines().any(|l| l.starts_with("UU")) && !repo.path.join("conf.txt").exists()
                || std::fs::read_to_string(repo.path.join("conf.txt"))
                    .map(|s| s.starts_with("MAIN-one"))
                    .unwrap_or(false)
        }),
        "Abort merge must restore pre-merge state"
    );
}

/// RED: Continue merge completes the merge once zero conflicts remain.
#[test]
fn continue_merge_completes_when_no_conflicts_remain() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "continue-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Resolve every conflict in the file. After resolving conflict 1 the
    // active cursor stays on it (Auto-advance is off by default); we
    // navigate to conflict 2 and then resolve it. That second side-take
    // happens on a file that now carries merged changes (conflict 1 is
    // resolved), so the Overwrite Changes pre-check holds it and the
    // dangerous affirmative is what applies it.
    h.get_by_label("Take ours 1").click();
    h.run();
    h.get_by_label("Next >").click();
    h.run();
    h.get_by_label("Take ours 2").click();
    h.run();
    h.get_by_label("Discard and Accept").click();
    h.run();

    // Apply writes the composed buffer to the working tree and stages it,
    // clearing the unmerged state; only then can `git merge --continue`
    // succeed.
    h.get_by_label("Apply").click();
    h.run();

    // The composed buffer is the file content; the in-progress merge has
    // no conflicts left in the index but the MERGE_HEAD is still pending.
    h.get_by_label("Continue merge").click();
    h.run();

    assert!(
        wait_until(10_000, || {
            let status = git(&repo.path, &["status", "--porcelain"]);
            status.trim().is_empty()
        }),
        "Continue merge must finish once no conflicts remain"
    );
}

/// RED: The three panes are equal width with discrete tinted conflict blocks
/// (the redesign preserves the EQUAL layout from issue #15).
#[test]
fn resolver_renders_equal_width_panes_with_tinted_blocks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "panes2-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Three pane headers paint.
    assert_painted(&h, "Local (Yours)");
    assert_painted(&h, "Result");
    assert_painted(&h, "Incoming (Theirs)");

    // Both sides of the first conflict block paint.
    assert_painted(&h, "MAIN-one");
    assert_painted(&h, "SIDE-one");

    // The three panes are EQUAL width.
    let local = header_band(&h, "Local (Yours)");
    let result = header_band(&h, "Result");
    let incoming = header_band(&h, "Incoming (Theirs)");
    assert!(
        (local.width() - result.width()).abs() <= 6.0
            && (result.width() - incoming.width()).abs() <= 6.0,
        "panes must be equal width, got widths {}/{}/{}",
        local.width(),
        result.width(),
        incoming.width()
    );

    // Yours/theirs sections are tinted behind their block text.
    let ours_pos = origin_of(&h, "MAIN-one").expect("ours text painted");
    let theirs_pos = origin_of(&h, "SIDE-one").expect("theirs text painted");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == yours_bg() && r.contains(ours_pos)),
        "yours section must be tinted behind its text"
    );
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == theirs_bg() && r.contains(theirs_pos)),
        "theirs section must be tinted behind its text"
    );

    // Marker strips paint as warning-tinted bands.
    assert!(
        filled_rects(&h).iter().any(|(_, c)| *c == marker_bg()),
        "conflict marker strips must be painted"
    );

    // The shared kit keeps the focused Result treatment on the header — as R1's
    // 2pt accent rail at the pane's leading edge, painted by the one rail
    // painter — and keeps the BRAND focus outline on the inactive read-only
    // cell, which really is a floating editor.
    let result_header_pos = galley_origin(&h, "Result").expect("Result header painted");
    assert!(
        filled_rects(&h).iter().any(|(r, c)| {
            *c == Palette::BRAND
                && (r.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && r.right() <= result_header_pos.x
                && r.top() <= result_header_pos.y
                && result_header_pos.y <= r.bottom()
        }),
        "resolver Result header must carry the shared 2pt BRAND focus rail beside \
         it, at {result_header_pos:?}: {:?}",
        filled_rects(&h)
            .into_iter()
            .filter(|(_, c)| *c == Palette::BRAND)
            .collect::<Vec<_>>()
    );
    let inactive_result_pos =
        origin_of(&h, "<< unresolved >>").expect("inactive Result placeholder painted");
    assert!(
        stroked_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::BRAND && r.contains(inactive_result_pos)),
        "inactive resolver Result cell must carry the shared BRAND focus outline"
    );
}

#[test]
fn resolver_panes_and_controls_remain_reachable_at_small_window() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "small-window-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    let size = egui::vec2(720.0, 640.0);
    h.set_size(size);
    h.run_steps(4);
    let viewport = Rect::from_min_size(Pos2::ZERO, size);

    for label in ["Result", "Take ours 1", "Apply"] {
        h.get_by_label(label).scroll_to_me();
        h.run_steps(4);
        let rect = h.get_by_label(label).rect();
        assert!(
            viewport.intersects(rect),
            "{label} must remain reachable at 720x640; rect={rect:?}, viewport={viewport:?}"
        );
    }
}

/// The Overwrite Changes pre-check. `Take ours` / `Take theirs`
/// are the resolver's Accept Yours / Accept Theirs; on a file that already
/// carries merged changes (any conflict resolved, or the Result free-edited)
/// they confirm first. The trigger is per FILE — the reference's
/// "This file already contains merged changes" — assumed per file by the
/// ticket (UNVERIFIED against the running product). The affirmative button is
/// the dangerous sentence (`Discard and Accept`); the safe option does
/// nothing.
///
/// Both title strings are asserted **inside the confirm's own window rect**:
/// the resolver's file list and body reuse enough words that an unscoped
/// substring assert would match them.
#[test]
fn accept_side_on_file_with_merged_changes_confirms_overwrite_first() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "overwrite-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Fresh file — no merged changes: Accept Yours resolves directly and no
    // confirm paints.
    h.get_by_label("Take ours 1").click();
    h.run();
    assert_eq!(h.state().ui.conflict_res[0], Some(0));
    assert_not_painted(&h, "Overwrite Changes");
    assert_not_painted(&h, "Discard and Accept");

    // The file now carries merged changes: Accept Theirs (the compact row of
    // the still-unresolved conflict 2) confirms first.
    h.get_by_label("Take theirs").click();
    h.run();
    let confirm = h.get_by_role_and_label(Role::Window, "Overwrite Changes");
    let rect = confirm.rect();
    let galleys = painted_galleys(&h);
    let scoped = |needle: &str| {
        galleys
            .iter()
            .find(|g| g.text == needle)
            .unwrap_or_else(|| panic!("`{needle}` was not painted by the confirm"))
    };
    for needle in ["Overwrite Changes", "Discard and Accept"] {
        let g = scoped(needle);
        assert!(
            rect.contains(g.pos),
            "`{needle}` must paint inside the confirm window (rect {rect:?}), got {g:?}"
        );
    }

    // The safe option does nothing: the pending take is dropped, the
    // conflict stays unresolved, the confirm closes.
    h.get_by_label("Keep Merged Changes").click();
    h.run();
    assert_eq!(
        h.state().ui.conflict_res[1],
        None,
        "the safe option must not resolve anything"
    );
    assert_not_painted(&h, "Overwrite Changes");

    // The dangerous affirmative IS the accept: it applies the pending side.
    h.get_by_label("Take theirs").click();
    h.run();
    h.get_by_label("Discard and Accept").click();
    h.run();
    assert_eq!(
        h.state().ui.conflict_res[1],
        Some(1),
        "Discard and Accept must apply the accepted side"
    );
    assert_not_painted(&h, "Overwrite Changes");
}

/// The confirm's two options are *visually* distinguishable, not
/// two identical rects: the dangerous affirmative wears the primary kit idle
/// fill ([`Palette::ACCENT`], what `KitButton::Primary` already paints), and
/// the safe option wears the raised-on-card surface (`Palette::SURFACE_2`).
#[test]
fn overwrite_confirm_affirmative_and_safe_option_paint_different_fills() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "overwrite-fills-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);
    h.get_by_label("Take ours 1").click();
    h.run();
    h.get_by_label("Take theirs").click();
    h.run();

    let discard_pos = galley_origin(&h, "Discard and Accept")
        .unwrap_or_else(|| panic!("Discard and Accept was not painted"));
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::ACCENT && r.contains(discard_pos)),
        "Discard and Accept must idle-fill ACCENT behind {discard_pos:?}"
    );
    let keep_pos = galley_origin(&h, "Keep Merged Changes")
        .unwrap_or_else(|| panic!("Keep Merged Changes was not painted"));
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::SURFACE_2 && r.contains(keep_pos)),
        "the safe option must idle-fill SURFACE_2 behind {keep_pos:?}"
    );
}

/// The folded bulk control. TurboGit deliberately folds the
/// reference's `Apply Non-Conflicting Changes` / `Resolve All Simple
/// Conflicts` pair into one `Apply to all N remaining` control, so with a
/// conflict present (the class the reference calls "simple") neither
/// reference string ever paints. The scope option set to *ours* resolves
/// every remaining conflict from the left side — the right side's hunk
/// content stops painting while the left's remains, asserted from geometry
/// (the Incoming header band marks the right column), because the resolution
/// state is a marker, not a word.
#[test]
fn bulk_apply_scope_resolves_remaining_from_one_side_without_reference_strings() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "scope-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Resolve conflict 1 with the RIGHT side first: now the active choice
    // is theirs, so only the scope option can make the bulk fill resolve
    // the rest from the left. (On a fresh file this take resolves directly —
    // no merged changes yet.)
    h.get_by_label("Take theirs 1").click();
    h.run();

    // Scope the bulk apply to the left side, then fill the remaining one.
    // egui's radio exposes both a Button and a RadioButton node under the
    // same label — query by role.
    h.get_by_role_and_label(Role::RadioButton, "Ours").click();
    h.run();
    h.get_by_label("Apply to all 1 remaining").click();
    h.run();

    assert_eq!(
        h.state().ui.conflict_res,
        vec![Some(1), Some(0)],
        "scope ours must fill the remaining conflict from the left side, \
         not from the active conflict's theirs choice"
    );
    assert_not_painted(&h, "Resolve All Simple Conflicts");
    assert_not_painted(&h, "Apply non-conflicting");

    // The resolution state is a marker, not a word: the sides each choice
    // kept/dropped are asserted from geometry. Conflict 1 kept the right
    // side (its `MAIN-one` hunk stops painting); conflict 2 kept the left
    // (its `SIDE-nine` hunk stops painting).
    assert!(
        origin_of(&h, "MAIN-one").is_none(),
        "the first conflict kept theirs, so its ours hunk must stop painting"
    );
    assert!(
        origin_of(&h, "SIDE-nine").is_none(),
        "the second conflict kept ours, so its theirs hunk must stop painting"
    );

    // What each side kept paints inside that side's own column. The band
    // hairline is inset by the pane header's 8pt air while a side section's
    // text is inset 6pt, so a kept hunk legitimately starts up to that much
    // left of the band — the right edge is exact.
    let local_band = header_band(&h, "Local (Yours)");
    let incoming_band = header_band(&h, "Incoming (Theirs)");
    let column_left = |band: Rect| band.left() - 8.0;
    let kept_theirs = origin_of(&h, "SIDE-one")
        .unwrap_or_else(|| panic!("conflict 1's kept theirs hunk must remain painted"));
    assert!(
        kept_theirs.x >= column_left(incoming_band) && kept_theirs.x <= incoming_band.right(),
        "the kept theirs hunk must sit in the right column (band {incoming_band:?}), got {kept_theirs:?}"
    );
    let kept_ours = origin_of(&h, "MAIN-nine")
        .unwrap_or_else(|| panic!("conflict 2's kept ours hunk must remain painted"));
    assert!(
        kept_ours.x >= column_left(local_band) && kept_ours.x <= local_band.right(),
        "the kept ours hunk must sit in the left column (band {local_band:?}), got {kept_ours:?}"
    );
}

/// PARITY: the two surfaces interpret the same choices to the same bytes.
///
/// The two surfaces used to carry their own character-identical copy of the
/// composer, which is exactly why nothing compared them — the agreement was
/// guaranteed by hand and invisible to every suite. One seeded repository,
/// the same two choices, taken through the resolver and then through the
/// inline editor: both buffers must land the bytes this suite spells out.
///
/// The second side-take on each surface lands on a file that already carries
/// merged changes, so the overwrite confirm's dangerous affirmative is what
/// applies it there.
#[test]
fn both_surfaces_compose_the_same_bytes_for_the_same_choices() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "parity-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Resolver: Take ours 1, Take theirs 2.
    open_resolver(&mut h);
    h.get_by_label("Take ours 1").click();
    h.run();
    h.get_by_label("Next >").click();
    h.run();
    h.get_by_label("Take theirs 2").click();
    h.run();
    h.get_by_label("Discard and Accept").click();
    h.run();
    let resolver_text = h.state().ui.conflict_text.clone();

    // Inline editor, same repository, same two choices. The resolver is
    // dismissed first: the two tool windows are not meant to be open at once,
    // and its window area would otherwise swallow the click on the changelist
    // entry behind it. Opening the inline editor reseeds the shared state from
    // the file on disk, so this is a claim about the composer rather than
    // about whichever choices survived.
    h.state_mut().ui.conflict_resolver_open = false;
    h.run();
    h.get_by_label("Merge…").click();
    h.run();
    h.get_by_label("Accept Yours 1").click();
    h.run();
    h.get_by_label("Accept Theirs 2").click();
    h.run();
    let inline_text = h.state().ui.conflict_text.clone();

    assert_eq!(
        resolver_text, PARITY_COMPOSED,
        "the resolver must compose ours-then-theirs"
    );
    assert_eq!(
        inline_text, PARITY_COMPOSED,
        "the inline editor must compose ours-then-theirs"
    );
    assert_eq!(
        resolver_text, inline_text,
        "the two surfaces must compose the same choices to the same bytes"
    );
}

/// The re-duplication ratchet. The fold that concentrated the editor core into
/// `ui::conflicts` deleted `conflict_resolver`'s private `CHOICE_*` ordinals,
/// its `merge_editor_segments` and its `compose`; nothing else in the tree
/// compares the two modules, and the copy this refuses was character-identical
/// to its neighbour, so no render assertion can tell them apart. Read the
/// source at compile time and refuse the declaration, not the mention — the
/// resolver still *names* all of these, as imports and call sites.
///
/// The scan reads top-level items by their rustfmt column-0 head, which is
/// sound here only because `cargo fmt -- --check` is a repository gate: a
/// hand-rolled layout panics rather than passing quietly.
#[test]
fn resolver_declares_no_editor_core_of_its_own() {
    let declared: Vec<String> = srcscan::top_level_items(RESOLVER_SRC)
        .into_iter()
        .map(|item| item.name)
        .collect();

    assert!(
        !declared.iter().any(|name| name == "compose"),
        "`conflict_resolver` declares its own `compose` again — the fold that moved the \
         composer into `ui::conflicts` deleted this copy, and a second copy is the \
         duplication coming back"
    );
    assert!(
        !declared.iter().any(|name| name == "merge_editor_segments"),
        "`conflict_resolver` declares its own `merge_editor_segments` again — the fold \
         that moved the segment reader into `ui::conflicts` deleted this copy, and a \
         second copy is the duplication coming back"
    );
    let choice_consts: Vec<&String> = declared
        .iter()
        .filter(|name| name.starts_with("CHOICE_"))
        .collect();
    assert!(
        choice_consts.is_empty(),
        "`conflict_resolver` declares its own choice ordinals {choice_consts:?} — the \
         fold that moved the `CHOICE_*` constants into `ui::conflicts` deleted these \
         copies, and a second set is the duplication coming back"
    );
}
