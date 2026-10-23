//! Issue #13 — diff viewer restyle: modes, tokens.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] over real
//! temp repositories (system `git`) seeded with staged/unstaged edits.
//! Asserts only on public surfaces:
//!
//! - **Painted output** — text galleys carry their strings; filled rects
//!   carry geometry + token color (`Palette::DIFF_*`).
//! - **State transitions** — public `AppState` fields after the frames.
//!
//! Covered (spec §8.4):
//! - segmented control toggles side-by-side/unified rendering
//! - hunk nav ‹ n/N › counts and steps correctly
//! - hunk nav aims the hunk it names, from any scroll position, in both modes
//! - The whitespace policy stops painting the line pair it calls equal
//! - add/del rows paint token-exact backgrounds; gutters show muted numbers
//! - the row's staged-ness reaches both render modes, both cells of a
//!   side-by-side pair, and the gutter's hover step
//! - a conflicted path claims no state in paint, in the header, or in words
//! - a row's staged-ness label follows the click that moves the index, while the
//!   patch on screen does not move at all
//! - the diff's ghost icon buttons (nav pair, gutter pair) are two scale
//!   settings of one shared primitive and keep their labels and enabled flags
//! - no surface offers a comparison-axis chip group: the pane answers one fixed
//!   read, so `Repo`, `Staged` and `Local` never appear as mode selectors
//! - the add/remove counts and the whitespace toggle paint on the file row, and
//!   the toolbar paints neither
//! - the file row's band height and its label origin are the row shell's
//! - the synthesised rename header is one muted band across the row
//!
//! **Ticket 12's "no diff token changed" assertion now lives in
//! `tests/design_tokens.rs`**, with the rest of the token contract: it pins six
//! diff values by hex, measures both text-on-band pairs against AA, and asserts
//! the pane paints them. It was here first because this file was another agent's
//! partition while that ticket was in flight; the token contract is where a
//! palette belongs, and this suite keeps the diff's *behaviour*.

use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Color32, Pos2, Rect, Vec2};
use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_galleys, painted_text,
    shell_harness_over, widget_harness,
};
use test_support::wait_until;
use turbogit_app::events::AppEvent;
use turbogit_app::keyed_read::{DiffTarget, Keyed};
use turbogit_app::state::AppState;
use turbogit_domain::error::TgError;
use turbogit_domain::model::{ChangeRef, RootId};
use turbogit_ui::theme::Palette;

// --- git seeding -------------------------------------------------------------

fn write_file(repo: &Path, name: &str, content: &str) {
    std::fs::write(repo.join(name), content).expect("writing file");
}

/// An initialized repository on `main` with **no commit at all** — the floor every
/// builder writes its own first commit into. Not a `git_seed` recipe: its committed
/// file would be swept into each builder's `git add .` and these assert on file sets.
fn init_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    (tmp, repo)
}

/// Repo where `file.txt` carries BOTH a staged edit (line 2 `beta`→`BETA`)
/// and an unstaged edit (line 8 `gamma`→`GAMMA`). The edits sit far enough
/// apart that each of git's three diffs shows its own change without the
/// other's content leaking into surrounding context lines:
///
/// - `git diff HEAD`    (HEAD↔worktree):  `BETA` and `GAMMA`, the pinned read
/// - `git diff --cached` (HEAD↔index):     `BETA` only
/// - `git diff`          (index↔worktree): `GAMMA` only
fn repo_mixed() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(
        &repo,
        "file.txt",
        "alpha\nbeta\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    // Staged edit: beta → BETA.
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    git(&repo, &["add", "file.txt"]);
    // Unstaged edit on top: gamma → GAMMA.
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\nGAMMA\niota\n",
    );
    (tmp, repo)
}

/// Repo where `one.txt` is fully staged (no unstaged part): the pinned read has
/// nothing left to show for it.
fn repo_staged_only() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "one.txt", "one\ntwo\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "one.txt", "one\nTWO\n");
    git(&repo, &["add", "one.txt"]);
    (tmp, repo)
}

/// Repo whose only change is a whitespace-only unstaged edit
/// (`gamma` → `g amma`): under `Ignore whitespaces` the pair becomes one
/// unchanged line, and under `Trim whitespaces` — which reads the same fixture —
/// it stays the difference it is.
fn repo_whitespace() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "ws.txt", "alpha\nbeta\ngamma\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "ws.txt", "alpha\nbeta\ng amma\n");
    (tmp, repo)
}

/// Repo with two separated unstaged edits in `nav.txt` (lines 2 and 12),
/// producing exactly two hunks for navigation tests.
fn repo_two_hunks() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=16).map(|i| format!("l{i:02}")).collect();
    write_file(&repo, "nav.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[1] = "X2".to_string();
    edited[11] = "X12".to_string();
    write_file(&repo, "nav.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

/// Two commits, so a commit-to-commit target (`HEAD~1` ↔ `HEAD`) has real
/// content to render. Working-tree-only controls (the comparison axis) have
/// nothing to say about such a comparison.
fn repo_two_commits() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=10).map(|i| format!("l{i:02}")).collect();
    write_file(&repo, "hist.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[2] = "SECOND".to_string();
    write_file(&repo, "hist.txt", &format!("{}\n", edited.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c2"]);
    (tmp, repo)
}

/// A commit that `git` reports as a **rename with content edits** — similarity
/// below 100%, so the pane keeps its scroller and the synthesised rename header
/// renders as a row of it rather than taking the pure-rename shortcut.
///
/// Committed, and therefore read through the commit-to-commit target: rename
/// detection needs BOTH sides of the change in the diff, so a path-limited
/// working-tree diff of the renamed file can only ever report it as a new file.
fn repo_renamed_with_edit() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let body: Vec<String> = (1..=12).map(|i| format!("line{i:02}")).collect();
    write_file(&repo, "old.txt", &format!("{}\n", body.join("\n")));
    write_file(&repo, "keep.txt", "untouched\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    git(&repo, &["mv", "old.txt", "new.txt"]);
    let mut moved = body;
    moved[3] = "line04 edited".to_string();
    write_file(&repo, "new.txt", &format!("{}\n", moved.join("\n")));
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-m", "c2"]);
    (tmp, repo)
}

/// Run `git <args>` in `dir` without asserting success — a merge that
/// conflicts exits non-zero, which is exactly the state under test. Kept local
/// because it is the opposite contract from `git_seed::git`.
fn git_expect_failure(dir: &Path, args: &[&str]) {
    let _ = Command::new("git").args(args).current_dir(dir).output();
}

/// Repo left mid-merge on `conf.txt`, so it reports
/// `ChangeStatus::Conflicted` and its hunk-header gutter pair renders
/// visible-but-disabled.
fn repo_conflicted() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "conf.txt", "one\ntwo\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    write_file(&repo, "conf.txt", "one\nside\n");
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-m", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
    write_file(&repo, "conf.txt", "one\nmain line\n");
    git(&repo, &["add", "conf.txt"]);
    git(&repo, &["commit", "-m", "main line"]);
    git_expect_failure(&repo, &["merge", "--no-edit", "side"]);
    (tmp, repo)
}

// --- harness -----------------------------------------------------------------

/// Harness rendering the full shell over a real repository. Mirrors
/// production setup (`app.rs`): dark tokens every frame, fonts installed
/// once, and the worker-event channel drained every frame before render.
fn diff_harness(repo: &Path) -> Harness<'static, AppState> {
    let state = AppState::new(repo.to_path_buf());
    assert!(
        !state.multi.roots.is_empty(),
        "repo root must be discovered"
    );
    // Tall enough that the shell frame (issue #03: center tabs + status bar —
    // nothing claims the top edge) leaves the preview column room for the
    // tallest comparison diff plus the message editor below it.
    shell_harness_over(state, egui::vec2(1024.0, 900.0))
}

#[test]
fn diff_renders_its_keyed_read_waiting_message_through_the_shared_presenter() {
    let (_tmp, repo) = repo_two_hunks();
    let mut harness = diff_harness(&repo);
    harness.state_mut().ui.preview_change = Some(ChangeRef::new(
        RootId(repo.to_path_buf().into()),
        repo.join("nav.txt"),
    ));
    harness.step();
    assert_painted(&harness, "Computing diff…");
}

#[test]
fn diff_renders_its_keyed_read_failure_message_through_the_shared_presenter() {
    let (_tmp, repo) = repo_two_hunks();
    let mut harness = diff_harness(&repo);
    let path = repo.join("nav.txt");
    harness.state_mut().ui.preview_change = Some(ChangeRef::new(
        RootId(repo.to_path_buf().into()),
        path.clone(),
    ));
    let target = DiffTarget::working_tree(
        repo.clone(),
        turbogit_domain::model::DiffPolicies::default(),
        Some(path),
    );
    harness
        .state_mut()
        .tx
        .send(AppEvent::DiffReady {
            key: target.key(),
            policies: turbogit_domain::model::DiffPolicies::default(),
            result: Err(TgError::Other("diff read failed".into())),
        })
        .expect("send diff failure");
    harness.step();
    assert_painted(&harness, "diff read failed");
}

/// Step frames until painted output AND the diff read's activity stabilize
/// (the fingerprint includes the read's verdict so a late `DiffReady` can never
/// be mistaken for a settled frame). Budgeted by wall-clock time — not frame
/// count — so a contended `git` subprocess cannot starve it.
/// A **third** settle family: neither shared settle looks at `read_pending`, and
/// a frame whose text is byte-identical while a `DiffReady` is in flight would
/// report the loading chrome as a settled diff. Folding this in turns the
/// assertions into races.
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
    panic!("diff viewer did not settle within 15s");
}

/// Open the Commit tab's inline preview for one file of one repository (public
/// state transition — the same thing clicking a row's Diff button does).
fn open_preview(harness: &mut Harness<'_, AppState>, root: &Path, file: &Path) {
    harness.state_mut().ui.preview_change = Some(ChangeRef::new(
        RootId(root.to_path_buf().into()),
        file.to_path_buf(),
    ));
    settle(harness);
}

// --- Cycle 1: one pinned read, and no control that moves it -----------------

/// Contract: the pane answers with `git diff HEAD` for a file carrying both a
/// staged and an unstaged edit, and nothing on screen can put another pair of
/// sides under it. `GAMMA` is the unstaged edit, `BETA` the staged one: the one
/// fixed comparison shows both halves, which is the whole reason it replaced the
/// chip group. The worktree's text and HEAD's text are on screen at the same time.
#[test]
fn the_pane_answers_with_git_diff_head_and_shows_both_halves() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    assert_painted(&h, "GAMMA");
    assert_painted(&h, "gamma");
    for staged_side in ["beta", "BETA"] {
        assert_painted(&h, staged_side);
    }

    let combined = git(&repo, &["diff", "HEAD", "--", "file.txt"]);
    let plain = git(&repo, &["diff", "--", "file.txt"]);
    assert!(
        combined.contains("-beta")
            && combined.contains("+BETA")
            && combined.contains("-gamma")
            && combined.contains("+GAMMA"),
        "the read the pane is pinned to carries all four rows:\n{combined}"
    );
    assert!(
        plain.contains("+GAMMA") && !plain.contains("BETA"),
        "git's unstaged diff is the narrower one, and the pane is not showing it:\n{plain}"
    );
}

/// The pinned read reaches HEAD: the line only HEAD holds is on screen as a
/// removal, whatever the index did with it afterwards. This runs the in-process
/// producer and grades it against git's own `git diff HEAD`.
#[test]
fn the_pinned_read_reaches_head() {
    for original in ["", "HEAD ONLY\n"] {
        let (_tmp, repo) = init_repo();
        write_file(&repo, "file.txt", original);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "base"]);
        write_file(&repo, "file.txt", "INDEX ONLY\n");
        git(&repo, &["add", "file.txt"]);
        write_file(&repo, "file.txt", "WORKTREE ONLY\n");

        let mut h = diff_harness(&repo);
        assert_eq!(
            h.state().settings.backend,
            turbogit_domain::model::GitBackend::InProcessReads
        );
        assert!(h.state().settings.in_process_diffs);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while h.query_by_label("file.txt").is_none() {
            h.step();
            assert!(
                std::time::Instant::now() < deadline,
                "file row did not load"
            );
        }
        h.get_by_label("file.txt").click();
        settle(&mut h);
        assert_eq!(
            h.state().ui.preview_change,
            Some(ChangeRef::new(
                RootId(repo.to_path_buf().into()),
                "file.txt"
            ))
        );

        assert_painted(&h, "WORKTREE ONLY");
        if original.is_empty() {
            assert_not_painted(&h, "HEAD ONLY");
        } else {
            assert_painted(&h, "HEAD ONLY");
        }
        assert_no_comparison_chips(&h, "the in-process preview");
        let combined = git(&repo, &["diff", "HEAD", "--", "file.txt"]);
        if original.is_empty() {
            assert!(
                combined.contains("+WORKTREE ONLY") && !combined.contains("HEAD ONLY"),
                "an empty HEAD leaves only the worktree's line:\n{combined}"
            );
        } else {
            assert!(
                combined.contains("-HEAD ONLY") && combined.contains("+WORKTREE ONLY"),
                "git's own HEAD↔worktree answer for this fixture:\n{combined}"
            );
        }
    }
}

/// A file with nothing unstaged still has everything the combined read shows: its
/// staged half is on screen, where an `index ↔ worktree` read would have answered
/// `(no differences)`.
#[test]
fn a_fully_staged_file_still_answers_the_combined_read() {
    let (_tmp, repo) = repo_staged_only();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("one.txt"));

    assert_painted(&h, "TWO");
    assert_painted(&h, "two");
    assert!(
        !painted_text(&h)
            .iter()
            .any(|t| t.contains("(no differences)")),
        "the read that hides staged content is gone"
    );
    assert!(
        git(&repo, &["diff", "--", "one.txt"]).is_empty(),
        "the fixture leaves nothing for `git diff` to answer, and the pane shows \
         the file anyway"
    );
    assert_no_comparison_chips(&h, "a fully staged file's preview");
}

// --- Cycle 2: segmented mode control -----------------------------------------

#[test]
fn segmented_control_toggles_side_by_side_and_unified() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    // Unified is the default: no side-by-side pane headers.
    assert!(!h.state().ui.diff_side_by_side);
    assert_not_painted(&h, "After Working tree");

    h.get_by_label("Side-by-Side").click();
    settle(&mut h);
    assert!(h.state().ui.diff_side_by_side);
    // Pane headers document the pinned pair (HEAD ↔ working tree).
    assert_painted(&h, "Before HEAD");
    assert_painted(&h, "After Working tree");

    h.get_by_label("Unified").click();
    settle(&mut h);
    assert!(!h.state().ui.diff_side_by_side);
    assert_not_painted(&h, "After Working tree");
}

// --- Cycle 3: hunk navigation -------------------------------------------------

#[test]
fn hunk_navigation_counts_and_steps_across_two_hunks() {
    let (_tmp, repo) = repo_two_hunks();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("nav.txt"));

    // Counter shows position 1 of 2.
    assert_painted(&h, "1/2");
    assert_eq!(h.state().ui.diff_current_hunk, 0);

    h.get_by_label("Next hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 1);
    assert_painted(&h, "2/2");

    // Next at the last hunk stays put.
    h.get_by_label("Next hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 1);

    h.get_by_label("Previous hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 0);
    assert_painted(&h, "1/2");

    // Previous at the first hunk stays put.
    h.get_by_label("Previous hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 0);
}

// --- Cycle 4: the whitespace policy ------------------------------------------

/// The `Ignore Differences` policy reaches the **comparison**, not only its
/// colouring: under `Ignore whitespaces` the two sides are the same line, so the
/// old side's row stops being painted and one unchanged row stands where the pair
/// was. The boolean checkbox this replaces asked the question of `git` (a `-w`
/// flag), which answered with an empty comparison; the policy is a view decision
/// folded into the display model, and the raw patch staging composes from keeps
/// both lines (`crates/turbogit-app/tests/diff_policies.rs` pins that half).
#[test]
fn whitespace_policy_stops_painting_the_line_pair_it_calls_equal() {
    use turbogit_domain::model::DiffWhitespace;
    let (_tmp, repo) = repo_whitespace();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("ws.txt"));

    // The menu the five values live in replaces the checkbox, on the file row.
    assert_painted(&h, "Diff options");
    assert_eq!(h.state().ui.diff_policies.whitespace, DiffWhitespace::None);
    assert_painted(&h, "gamma");
    assert_painted(&h, "g amma");

    h.state_mut().ui.diff_policies.whitespace = DiffWhitespace::IgnoreWhitespace;
    settle(&mut h);
    assert_not_painted(&h, "gamma");
    assert_painted(&h, "g amma");

    // `Trim whitespaces` is a *different* answer to this same diff: internal
    // whitespace is significant there, so the pair comes back. Two values that
    // behaved alike would be one value with two names.
    h.state_mut().ui.diff_policies.whitespace = DiffWhitespace::TrimWhitespace;
    settle(&mut h);
    assert_painted(&h, "gamma");

    // And back to the default, the full diff is painted again.
    h.state_mut().ui.diff_policies.whitespace = DiffWhitespace::None;
    settle(&mut h);
    assert_painted(&h, "gamma");
    assert_painted(&h, "g amma");
}

// --- Cycle 5: token-exact line styling ----------------------------------------

#[test]
fn add_del_rows_paint_token_backgrounds_with_muted_gutters() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    // The staged pair sits on the token bands, and the unstaged pair sits on the
    // muted roles beside them: four rows, two treatments, all painted in the same
    // frame. `repo_mixed` stages `beta` to `BETA` and leaves `gamma` to `GAMMA` in
    // the worktree, so the pane has no excuse for drawing them alike.
    let add_pos = galley_origin(&h, "GAMMA").expect("added line painted");
    let staged_add = galley_origin(&h, "BETA").expect("staged added line painted");
    for (who, at, band) in [
        ("unstaged added", add_pos, Palette::DIFF_ADD_UNSTAGED_BG),
        ("staged added", staged_add, Palette::DIFF_ADD_BG),
    ] {
        assert!(
            filled_rects(&h)
                .iter()
                .any(|(r, c)| *c == band && r.contains(at)),
            "{who} line must be backed by {band:?}"
        );
    }
    // …and the deleted lines on their own pair of bands.
    let del_pos = galley_origin(&h, "gamma").expect("deleted line painted");
    let staged_del = galley_origin(&h, "beta").expect("staged deleted line painted");
    for (who, at, band) in [
        ("unstaged deleted", del_pos, Palette::DIFF_DEL_UNSTAGED_BG),
        ("staged deleted", staged_del, Palette::DIFF_DEL_BG),
    ] {
        assert!(
            filled_rects(&h)
                .iter()
                .any(|(r, c)| *c == band && r.contains(at)),
            "{who} line must be backed by {band:?}"
        );
    }

    // Muted gutter numbers are painted alongside the changed lines
    // (new-file line number of the GAMMA/GAMMA hunk region).
    assert!(
        galley_origin(&h, "8").is_some(),
        "gutter line numbers must be painted"
    );

    // Hunk header band uses the SURFACE token.
    let hunk_header = painted_text(&h)
        .iter()
        .find(|t| t.starts_with("@@"))
        .cloned()
        .expect("hunk header painted");
    let hunk_pos = galley_origin(&h, &hunk_header).expect("hunk header origin");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::SURFACE && r.contains(hunk_pos)),
        "hunk header must be backed by SURFACE"
    );
}

// --- Cycle 6: one shared ghost icon button at two scales --------------------

/// Drive the shared ghost icon-button primitive directly at both scales a
/// host uses: the standard 24 px square (the diff nav control) and the dense
/// 18 px rect a gutter supplies itself. One enabled, one not.
fn ghost_primitive_harness() -> Harness<'static, ()> {
    use turbogit_ui::ui::icons::{self, Icon};
    use turbogit_ui::ui::widgets::{self, ButtonVariant};
    widget_harness(egui::vec2(240.0, 80.0), move |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
            widgets::ghost_icon_button(
                ui,
                rect,
                ui.id().with("Nav ghost"),
                "Nav ghost",
                true,
                |ui, rect, state| {
                    let ink = ButtonVariant::Ghost.text(state);
                    icons::centered_icon(ui, Icon::CHECK, rect.center(), 14.0, ink);
                },
            );
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
            widgets::ghost_icon_button(
                ui,
                rect,
                ui.id().with("Gutter ghost"),
                "Gutter ghost",
                false,
                |ui, _rect, state| {
                    let ink = ButtonVariant::Ghost.text(state);
                    icons::icon(ui, Icon::CHECK, 12.0, ink);
                },
            );
        });
    })
}

/// Contract: the diff's ghost icon controls — the hunk-nav pair and the
/// hunk-header gutter pair — are two scale settings of one general primitive
/// built over the shared button-state vocabulary. What must survive that
/// collapse is everything *not* in the ladder: the control is still findable
/// by its accessibility label, it still reports enabled/disabled honestly,
/// and a conflicted hunk's gutter control still reports disabled rather than
/// disappearing.
#[test]
fn ghost_icon_controls_keep_their_labels_and_enabled_flags() {
    // --- the general primitive itself, at both scales ---------------------
    let mut primitives = ghost_primitive_harness();
    primitives.step();

    assert!(
        !primitives
            .get_by_label("Nav ghost")
            .accesskit_node()
            .is_disabled(),
        "an enabled ghost icon button must report itself enabled"
    );
    assert!(
        primitives
            .get_by_label("Gutter ghost")
            .accesskit_node()
            .is_disabled(),
        "a disabled ghost icon button must report itself disabled"
    );

    // --- the migrated hosts, on a real diff ------------------------------
    let (_tmp, repo) = repo_two_hunks();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("nav.txt"));

    for label in ["Previous hunk", "Next hunk"] {
        assert!(
            !h.get_by_label(label).accesskit_node().is_disabled(),
            "the `{label}` nav control must stay findable by label and enabled \
             while the preview carries hunks"
        );
    }
    // The gutter pair on hunk 1 of a clean file is live.
    for label in ["Stage hunk 1", "Unstage hunk 1"] {
        assert!(
            !h.get_by_label(label).accesskit_node().is_disabled(),
            "the `{label}` gutter control must stay findable by label and \
             enabled on a clean hunk"
        );
    }

    // A conflicted hunk keeps the pair visible but inert.
    let (_tmp, conflicted) = repo_conflicted();
    let mut c = diff_harness(&conflicted);
    settle(&mut c);
    c.get_by_label("C conf.txt").click();
    settle(&mut c);
    assert!(
        !c.query_by_label("Stage hunk 1").is_none(),
        "conflicted files must still render their granular gutter controls"
    );
    assert!(
        c.get_by_label("Stage hunk 1")
            .accesskit_node()
            .is_disabled(),
        "a gutter control on a conflicted hunk must report disabled"
    );
}

// --- Cycle 7: hunk navigation aims the hunk it names -------------------------

/// A repo whose unstaged edit is `hunks` hunks of `changed` lines each, with
/// `gap` unchanged lines between them — a diff far taller than the preview pane,
/// and the shape hunk navigation needs: consecutive hunks are tens of rows
/// apart, so the list can be scrolled deep enough that the built row window
/// starts hundreds of rows down while a jump aims a hunk a few rows away.
fn repo_deep_diff(hunks: usize, changed: usize, gap: usize) -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let span = changed + gap;
    let mut base: Vec<String> = Vec::new();
    for i in 0..(hunks * span + gap) {
        base.push(format!("l{i:04}"));
    }
    write_file(&repo, "deep.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base.clone();
    for k in 0..hunks {
        for j in 0..changed {
            edited[k * span + j] = format!("X{k:02}{j:02}");
        }
    }
    write_file(&repo, "deep.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

/// The hunk header bands `git` itself reports for `deep.txt`, newest first, so
/// a test can name ONE hunk's header exactly as the pane paints it.
fn diff_hunk_headers(repo: &Path) -> Vec<String> {
    git(repo, &["diff", "-U3", "--", "deep.txt"])
        .lines()
        .filter(|l| l.starts_with("@@"))
        .map(str::to_owned)
        .collect()
}

/// Every hunk header the last frame painted.
fn painted_hunk_headers(harness: &Harness<'_, AppState>) -> Vec<String> {
    painted_text(harness)
        .into_iter()
        .filter(|t| t.starts_with("@@"))
        .collect()
}

/// Settle a frame that ISSUED a scroll. The aim is applied when the scroll area
/// ends the frame it was issued in, so the new offset first appears in the NEXT
/// frame's paint — and `settle` returns as soon as two frames agree, which it
/// can do on the frame before that. So a scroll is settled by stepping past it
/// first, then letting the layout settle.
fn settle_after_aim(harness: &mut Harness<'_, AppState>) {
    for _ in 0..3 {
        harness.step();
    }
    settle(harness);
}

#[track_caller]
fn assert_hunk_aimed_at(painted: &[String], want: &str, must_not_see: &[&str], what: &str) {
    assert!(
        painted.contains(&want.to_owned()),
        "{what}: the aimed hunk's header ({want}) is not on screen; painted \
         headers:\n{painted:#?}"
    );
    for away in must_not_see {
        assert!(
            !painted.contains(&away.to_string()),
            "{what}: the list is nowhere near the aimed hunk — {away} is on screen \
             instead; painted headers:\n{painted:#?}"
        );
    }
}

/// Aim three hunks in one open diff and check each one arrives on screen, the
/// list getting there from wherever the previous aim left it: the last hunk
/// (from the top), the hunk one step above it, and the first hunk.
///
/// `side_by_side` picks the rendering mode, because the two modes page over
/// DIFFERENT row streams over the same model and a hunk's row index is a slot
/// in one of them. Both modes are asked the same question here on purpose: an
/// aim that only works in one of them is a bug, and the mode names it.
fn hunk_aim_sequence(repo: &Path, side_by_side: bool) {
    const HUNKS: usize = 20;
    let headers = diff_hunk_headers(repo);
    assert_eq!(
        headers.len(),
        HUNKS,
        "the fixture's premise: {HUNKS} hunk headers to aim between"
    );
    let mode = if side_by_side {
        "side-by-side"
    } else {
        "unified"
    };

    let mut h = diff_harness(repo);
    h.state_mut().ui.diff_side_by_side = side_by_side;
    // Note what opening a diff costs the cursor: the keyed read's dispatch puts
    // `diff_current_hunk` back to 0, so the FIRST paint of this diff aims hunk 0
    // and spends that hunk's one aim. The near-the-top leg below therefore aims
    // hunk 1 — the next hunk down, still at the top of a 740-row diff — which is
    // also the one leg an "index space" bug cannot hide behind.
    settle(&mut h);
    open_preview(&mut h, repo, &repo.join("deep.txt"));
    assert_eq!(h.state().ui.diff_side_by_side, side_by_side);
    assert!(
        painted_hunk_headers(&h).len() < HUNKS,
        "the pane cannot show all {HUNKS} headers at once — the premise of this test"
    );

    // --- the last hunk, from wherever the list is resting ---
    h.state_mut().ui.diff_current_hunk = HUNKS - 1;
    settle_after_aim(&mut h);
    let deep = painted_hunk_headers(&h);
    assert_hunk_aimed_at(&deep, &headers[HUNKS - 1], &[&headers[0]], mode);
    assert!(
        deep.iter().all(|hd| {
            headers
                .iter()
                .position(|x| x == hd)
                .is_some_and(|k| k >= HUNKS / 2)
        }),
        "{mode}: every painted header should be in the bottom half of the diff after \
         aiming the last hunk, got {deep:?}"
    );

    // --- one hunk's step UP, from deep: the built row window now starts
    // hundreds of rows down, and the target is a few rows above the fold ---
    h.get_by_label("Previous hunk").click();
    settle_after_aim(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, HUNKS - 2);
    let stepped = painted_hunk_headers(&h);
    assert_hunk_aimed_at(
        &stepped,
        &headers[HUNKS - 2],
        &[&headers[0]],
        &format!("{mode}, one step up from deep"),
    );

    // --- and a hunk near the top, from down there ---
    h.state_mut().ui.diff_current_hunk = 1;
    settle_after_aim(&mut h);
    let top = painted_hunk_headers(&h);
    assert_hunk_aimed_at(
        &top,
        &headers[1],
        &[&headers[HUNKS - 1]],
        &format!("{mode}, a hunk near the top from deep"),
    );
}

/// Hunk navigation aims a row the list never built, so a row INDEX has to
/// become a position correctly whether the list is at the top or hundreds of
/// rows down — in the rendering mode the developer is actually in.
#[test]
fn hunk_navigation_aims_the_hunk_it_names_side_by_side() {
    let (_tmp, repo) = repo_deep_diff(20, 15, 20);
    hunk_aim_sequence(&repo, true);
}

/// The same three aims, in the unified mode the app opens in.
#[test]
fn hunk_navigation_aims_the_hunk_it_names_unified() {
    let (_tmp, repo) = repo_deep_diff(20, 15, 20);
    hunk_aim_sequence(&repo, false);
}

// =============================================================================
// Ticket 12 — the two axes of scope, and the statistics that sit with what they
// describe.
//
// Everything below is asserted from PAINTED GEOMETRY, never from which helper
// painted what. A grouped control is a shape (contiguous segments, one outer
// boundary), and a shape survives a rename; a name does not.
// =============================================================================

/// Half a pixel of slack: egui rounds UI rects, so exact float equality is the
/// wrong question to ask about two rects that are meant to coincide.
const EPS: f32 = 0.5;

/// The toolbar's open span — between the view-mode control and the hunk
/// navigator — which is precisely where the axes of scope live. Boundaries
/// found here belong to an axis; the view-mode control's own track is found and
/// excluded, so it cannot be counted as one.
fn axis_span(harness: &Harness<'_, AppState>) -> Rect {
    let mode = harness.get_by_label("Unified").rect();
    // The view-mode control's own track, by containment: the span starts where
    // that control ends.
    let track = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::SURFACE_2 && r.contains(mode.center()))
        .map(|(r, _)| r)
        .expect("the view-mode control paints its own track");
    let nav = harness.get_by_label("Previous hunk").rect();
    Rect::from_min_max(
        Pos2::new(track.right(), track.top().min(nav.top())),
        Pos2::new(nav.left(), track.bottom().max(nav.bottom())),
    )
}

/// The axis **group frames** in the toolbar's open span.
///
/// Two filters, and both matter. The horizontal one is strict: the view-mode
/// control's own track ends exactly where the span begins, and an inclusive test
/// would count that track as a second group. The height one is what makes this
/// "how many GROUPS are here" rather than "how many shapes are here": a group's
/// frame stands taller than the toolbar row, while the segments it groups are
/// row-height. An empty frame is a frame with nothing in it, so this count
/// answers the question the missing-axis case asks.
fn axis_groups(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    let span = axis_span(harness);
    let row_h = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| {
            *c == Palette::SURFACE_2 && r.contains(Pos2::new(span.left(), span.center().y))
        })
        .map(|(r, _)| r.height())
        .expect("the view-mode control's track");
    filled_rects(harness)
        .into_iter()
        .filter(|(_, c)| *c == Palette::SURFACE_2)
        .map(|(r, _)| r)
        .filter(|r| {
            r.left() > span.left() + EPS
                && r.right() < span.right() - EPS
                && r.height() > row_h + EPS
        })
        .collect()
}

/// Harness rendering ONLY the diff pane, over a commit-to-commit target — the
/// comparison the working-tree-only controls have nothing to say about.
///
/// Kept local as a recorded difference from `shell_harness_over`: it calls
/// `ui::diff::render_diff` directly, so the shell's other regions are not in the
/// frame — these assertions are about the diff pane alone.
fn commit_diff_harness(repo: &Path) -> Harness<'static, AppState> {
    let state = AppState::new(repo.to_path_buf());
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            let (left, right) = ("HEAD~1".to_owned(), "HEAD".to_owned());
            let root = state.multi.roots[0].id.clone();
            turbogit_ui::ui::diff::render_diff(ui, state, &root, &Some(left), &Some(right), &None);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 900.0));
    harness
}

/// The three arms of the deleted axis, asserted absent from one toolbar.
#[track_caller]
fn assert_no_comparison_chips(harness: &Harness<'_, AppState>, surface: &str) {
    for label in ["Repo", "Staged", "Local"] {
        assert!(
            harness.query_by_label(label).is_none(),
            "`{label}` was an arm of the deleted axis, so {surface} must not offer it"
        );
    }
}

/// Contract: the deleted axis leaves nothing behind on either surface. A
/// grouping that assumed a comparison pad still existed would paint an empty
/// frame in the toolbar's open span, and this says neither the working tree nor
/// a commit pair frames one, or offers a chip.
#[test]
fn the_removed_axis_leaves_no_group_anywhere() {
    // --- the working tree ---------------------------------------------------
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));
    assert_painted(&h, "GAMMA");
    let working_tree_groups = axis_groups(&h);
    assert_no_comparison_chips(&h, "the working-tree toolbar");
    assert_eq!(
        working_tree_groups,
        Vec::new(),
        "the working-tree toolbar frames no group now that the axis is gone"
    );

    // --- a commit pair -------------------------------------------------------
    let (_tmp, commits) = repo_two_commits();
    let mut c = commit_diff_harness(&commits);
    settle(&mut c);
    assert_painted(&c, "SECOND");
    let commit_groups = axis_groups(&c);
    assert_no_comparison_chips(&c, "a commit pair's toolbar");
    assert_eq!(
        commit_groups,
        Vec::new(),
        "a commit-to-commit toolbar frames no group either"
    );
}

/// Contract: the side-by-side headers name the sides the read actually takes.
/// A commit-to-commit diff prints its own two revisions. The axis hid this: the
/// labels were chosen from the chip, and `render_diff`'s one production caller
/// is the commit window's working-tree preview, so only a test reaches the
/// commit pair at all.
#[test]
fn a_commit_pair_prints_its_own_revisions_in_the_pane_headers() {
    let (_tmp, commits) = repo_two_commits();
    let mut c = commit_diff_harness(&commits);
    c.state_mut().ui.diff_side_by_side = true;
    settle(&mut c);
    assert_painted(&c, "Before HEAD~1");
    assert_painted(&c, "After HEAD");
    for working_tree_side in ["Before Index", "After Working tree"] {
        assert_not_painted(&c, working_tree_side);
    }
}

/// The file row is a row of the row shell's height and the row shell's text
/// origin, and gaining the statistics and the toggle did not change either.
/// Its fill is the one row-fill decision, so a file row is not a second
/// selection treatment.
fn hovered_file_row(harness: &mut Harness<'_, AppState>) -> (Rect, Pos2) {
    let galley = painted_galleys(harness)
        .into_iter()
        .find(|g| g.text.starts_with("diff --git "))
        .expect("the file row's own line is painted");
    // Inside the band, clear of the trailing zone and of the rounded corners.
    let point = Pos2::new(galley.pos.x + 1.0, galley.rect.center().y);
    harness.hover_at(point);
    harness.run();
    // Found by the row shell's OWN hover fill — not by a colour picked here —
    // so "the file row's fill is the one row-fill decision" is what makes this
    // find succeed at all.
    let hover_fill = turbogit_ui::ui::components::row_fill(
        turbogit_ui::ui::components::RowState::from_flags(false, true),
    );
    let band = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == hover_fill && r.contains(point))
        .map(|(r, _)| r)
        .expect("the hovered file row paints the row shell's hover fill");
    (band, point)
}

/// Contract: the statistics sit with the thing they describe. The counts and the
/// diff viewer's settings menu paint **on the file row**, and the diff pane's
/// toolbar paints neither of them any more.
///
/// Scoped by position, never by string: a `+1` is a perfectly ordinary thing for
/// another surface on screen to be painting (the commit window's preview card
/// header is still a legitimate place for it), so the question is not "how many
/// `+1`s are on screen" but "where are they".
#[test]
fn the_file_row_carries_the_counts_and_the_diff_options_menu() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    let (band, _) = hovered_file_row(&mut h);
    let toolbar = axis_span(&h);

    // The counts: this file's two additions and two deletions — the staged half
    // and the unstaged one, because the read is HEAD-based — in the stats accents,
    // inside the file row's band, and not in the toolbar.
    for (text, ink) in [
        ("+2", Palette::DIFF_ADD_ACCENT),
        ("\u{2212}2", Palette::DIFF_DEL_ACCENT),
    ] {
        let galleys: Vec<_> = painted_galleys(&h)
            .into_iter()
            .filter(|g| g.text == text)
            .collect();
        let on_row: Vec<_> = galleys
            .iter()
            .filter(|g| band.contains(g.rect.center()))
            .collect();
        assert_eq!(
            on_row.len(),
            1,
            "`{text}` is the file row's own statistic and must paint on the file \
             row exactly once: {on_row:?} of {galleys:?}"
        );
        assert_eq!(on_row[0].color, ink, "`{text}` keeps its stats accent");
        assert!(
            galleys.iter().all(|g| !toolbar.contains(g.rect.center())),
            "the diff toolbar no longer paints the statistics: `{text}` at {:?}",
            galleys
                .iter()
                .find(|g| toolbar.contains(g.rect.center()))
                .map(|g| g.rect)
        );
    }

    // The menu's trigger: one control, on the file row, nowhere in the toolbar.
    let toggles: Vec<Rect> = h
        .get_all_by_label("Diff options")
        .map(|n| n.rect())
        .collect();
    assert_eq!(
        toggles.len(),
        1,
        "the toolbar no longer paints the trigger; exactly one control remains"
    );
    assert!(
        band.contains(toggles[0].center()),
        "the policy menu belongs to the file row: {:?} is outside {band:?}",
        toggles[0]
    );
    assert!(
        !toolbar.intersects(toggles[0]),
        "the trigger moved off the toolbar onto the file row: {toggles:?}"
    );
}

/// Contract: the file row's height and leading padding are the ROW SHELL's,
/// after the new occupants are added. The band it paints under the pointer is
/// the same band every other diff row paints — the same height, the same full
/// width — and its label starts at the same text origin as the changed rows'
/// text. The occupant that would be hand-placed instead (a taller row, or a
/// second text origin) breaks one of these two.
#[test]
fn the_file_row_takes_its_geometry_from_the_row_shell() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    let (band, _) = hovered_file_row(&mut h);
    // The staged added row is the one that wears `DIFF_ADD_BG`; the unstaged row
    // beside it wears the muted role, and this claim is about the row shell's
    // geometry, which is the same for both.
    let add_pos = galley_origin(&h, "BETA").expect("added line painted");
    let add_band = filled_rects(&h)
        .into_iter()
        .find(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(add_pos))
        .map(|(r, _)| r)
        .expect("the added line's band");

    assert!(
        (band.height() - add_band.height()).abs() <= EPS,
        "the file row's band is {} tall, every other row's is {}: the height \
         belongs to the row shell, not to the file row",
        band.height(),
        add_band.height()
    );
    assert!(
        band.left() <= add_band.left() + EPS && band.right() >= add_band.right() - EPS,
        "the file row spans the row: {band:?} does not cover {add_band:?}"
    );
    let file_label = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text.starts_with("diff --git "))
        .expect("the file row's own line is painted");
    assert!(
        (file_label.pos.x - add_pos.x).abs() <= EPS,
        "the file row's label starts at x={} but every other row's text starts at \
         x={}: the leading padding belongs to the row shell",
        file_label.pos.x,
        add_pos.x
    );
}

/// Contract: the synthesised rename header reads as one muted band spanning the
/// row, and its text is painted exactly once inside it.
#[test]
fn the_rename_header_reads_as_one_muted_band() {
    let (_tmp, repo) = repo_renamed_with_edit();
    let mut h = commit_diff_harness(&repo);
    settle(&mut h);

    let headers: Vec<_> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("Renamed from "))
        .collect();
    assert_eq!(
        headers.len(),
        1,
        "the rename header is one band and paints its text once: {headers:?}"
    );
    let header = &headers[0];

    let bands: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, c)| *c == Palette::SECTION_BG)
        .map(|(r, _)| r)
        .filter(|r| r.contains(header.pos))
        .collect();
    assert_eq!(
        bands.len(),
        1,
        "one muted band, and the rename text is inside it: {bands:?}"
    );

    // ...and the band is the ROW's band, not a band shrunk around the text: the
    // same left edge, the same width, the same height as any other diff row.
    let edit_pos = galley_origin(&h, "line04 edited").expect("the edited line painted");
    let edit_band = filled_rects(&h)
        .into_iter()
        .find(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(edit_pos))
        .map(|(r, _)| r)
        .expect("the added line's band");
    let band = bands[0];
    assert!(
        (band.left() - edit_band.left()).abs() <= EPS
            && (band.width() - edit_band.width()).abs() <= EPS
            && (band.height() - edit_band.height()).abs() <= EPS,
        "the rename band must span the whole row the way every other diff row \
         does: {band:?} against a diff row's {edit_band:?}"
    );
}

/// Contract: the file row gained two occupants, and neither of them landed on
/// top of a row's own click targets. This is the failure mode the placement
/// risks — a trailing zone hung at the wrong coordinate silently eats a
/// changed row's click — so it is asserted against every band the rows paint.
#[test]
fn the_file_rows_occupants_do_not_overlap_the_rows_own_click_targets() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    let (band, _) = hovered_file_row(&mut h);
    let toggle = h.get_by_label("Diff options").rect();
    let changed: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, c)| {
            *c == Palette::DIFF_ADD_BG
                || *c == Palette::DIFF_DEL_BG
                || *c == Palette::DIFF_ADD_UNSTAGED_BG
                || *c == Palette::DIFF_DEL_UNSTAGED_BG
        })
        .map(|(r, _)| r)
        .collect();
    assert_eq!(
        changed.len(),
        4,
        "the staged pair and the unstaged pair are all on screen, each on its own \
         band: {changed:?}"
    );
    for row in &changed {
        assert!(
            !row.intersects(toggle),
            "the file row's menu trigger sits on a changed row's band {row:?} and \
             would swallow its click: {toggle:?}"
        );
    }
    for row in &changed {
        assert!(
            !row.intersects(band),
            "the file row's own band overlaps a changed row's band {row:?}: {band:?}"
        );
    }

    // The row's own protocol still answers: a changed-line click stages that
    // line, and git says so. Stepped, not run: the staging op repaints, and
    // this harness's run budget is kittest's default four. The row is addressed by
    // the label a changed line carries, its text and the state the per-line oracle
    // answers for it, which is `unstaged` right up until this click.
    h.get_by_label("GAMMA (unstaged)").click();
    assert!(
        wait_until(15_000, || {
            h.step();
            git(&repo, &["diff", "--cached"]).contains("+GAMMA")
        }),
        "the clicked line must reach the index, got:\n{}",
        git(&repo, &["diff", "--cached"])
    );
}

/// The diff actions module's own source, read at compile time — the ratchet
/// below is a claim about a function that must not exist in it.
const DIFF_ACTIONS_SRC: &str = include_str!("../src/ui/diff/actions.rs");

/// Contract: the previewed file's [`ChangeStatus`] is resolved at exactly one
/// address. `turbogit_app::granular::preview_status` owns the rule — the app's
/// off-frame diff read already routes through it — and the UI's frames call it
/// rather than carrying a second copy of the same walk. A copy that quietly
/// comes back would leave the question answered from two addresses in two
/// crates, each free to drift, and nothing but this scan would say so.
#[test]
fn the_diff_surface_declares_no_change_status_resolver_of_its_own() {
    assert!(
        !DIFF_ACTIONS_SRC.contains("fn preview_status"),
        "ui::diff::actions must not spell the change-status resolver: \
         turbogit_app::granular::preview_status owns it (lane 5, ticket 12), \
         and both diff frames call it"
    );
}

// --- staged-ness: where the treatment reaches, and what it never claims -------

/// Contract: the staged-ness treatment reaches **both cells** of a side-by-side
/// row, and every cell answers for itself. A pair's two halves sit side by side and
/// can be on opposite sides of the index (a staged modification re-edited in the
/// worktree answers its deletion one way and its addition the other), so the left
/// cell's band and the right cell's band come from two different answers. A pane
/// that painted the pair as one state would draw a staged half beside an unstaged
/// one and call them the same thing.
#[test]
fn side_by_side_paints_each_cells_own_staged_ness() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    h.state_mut().ui.diff_side_by_side = true;
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    for (who, band, ink) in [
        ("BETA", Palette::DIFF_ADD_BG, Palette::DIFF_ADD_TEXT),
        ("beta", Palette::DIFF_DEL_BG, Palette::DIFF_DEL_TEXT),
        (
            "GAMMA",
            Palette::DIFF_ADD_UNSTAGED_BG,
            Palette::DIFF_ADD_UNSTAGED_TEXT,
        ),
        (
            "gamma",
            Palette::DIFF_DEL_UNSTAGED_BG,
            Palette::DIFF_DEL_UNSTAGED_TEXT,
        ),
    ] {
        let at = galley_origin(&h, who).unwrap_or_else(|| panic!("`{who}` painted"));
        assert!(
            filled_rects(&h)
                .iter()
                .any(|(r, c)| *c == band && r.contains(at)),
            "the `{who}` cell must be backed by {band:?} in side-by-side mode too, \
             got {:?}",
            filled_rects(&h)
                .iter()
                .filter(|(r, _)| r.contains(at))
                .map(|(_, c)| *c)
                .collect::<Vec<_>>()
        );
        let ink_of = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == who)
            .map(|g| g.color);
        assert_eq!(
            ink_of,
            Some(ink),
            "the `{who}` cell inks its text with {ink:?}, its own state's ink"
        );
    }

    // And the two cells of one row are two different colours, because they are two
    // different git states sitting beside each other on one line.
    let del_band_of = |who: &str| {
        let at = galley_origin(&h, who).expect("row painted");
        filled_rects(&h)
            .iter()
            .find(|(r, c)| {
                r.contains(at)
                    && (*c == Palette::DIFF_DEL_BG || *c == Palette::DIFF_DEL_UNSTAGED_BG)
            })
            .map(|(_, c)| *c)
    };
    assert_ne!(
        del_band_of("beta"),
        del_band_of("gamma"),
        "a staged deletion and an unstaged deletion are two bands, not one"
    );
}

/// Contract: a muted row that is also hovered does not collapse into one
/// indistinguishable state.
///
/// The pointer answers in the gutter's line number, stepping `INK_3` to `INK_2`,
/// and the row's staged-ness keeps its own two colours underneath it. The app's
/// hover fill could not carry this: `SURFACE_2` against a muted band measures
/// 1.01:1, so an interaction fill laid over a muted row is invisible on exactly the
/// rows that need it, and a hover that brightened the row's ink instead would make
/// an unstaged row look staged the moment the pointer crossed it.
#[test]
fn hover_stays_visible_on_a_muted_row() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    // `GAMMA` is the worktree's line 8, and 8 is the number its gutter paints.
    let gutter_ink = |h: &Harness<'_, AppState>, row: Rect| {
        painted_galleys(h)
            .into_iter()
            .find(|g| g.text == "8" && row.contains(g.pos))
            .map(|g| g.color)
    };
    let row_ink = |h: &Harness<'_, AppState>| {
        painted_galleys(h)
            .into_iter()
            .find(|g| g.text == "GAMMA")
            .map(|g| g.color)
    };

    let row = h.get_by_label("GAMMA (unstaged)").rect();
    assert_eq!(
        gutter_ink(&h, row),
        Some(Palette::INK_3),
        "at rest the muted row's gutter is the muted step, and its code text is the \
         unstaged ink"
    );
    assert_eq!(
        row_ink(&h),
        Some(Palette::DIFF_ADD_UNSTAGED_TEXT),
        "the row starts quiet"
    );

    h.hover_at(row.center());
    h.step();
    h.step();
    let hovered = h.get_by_label("GAMMA (unstaged)").rect();
    assert_eq!(
        gutter_ink(&h, hovered),
        Some(Palette::INK_2),
        "hovered, the same row's gutter steps one ink rung, so muted-and-hovered is \
         not muted-and-rest"
    );
    assert_eq!(
        row_ink(&h),
        Some(Palette::DIFF_ADD_UNSTAGED_TEXT),
        "and the row is still muted under the pointer: hover answers where the \
         pointer is, not what git holds"
    );
    assert_ne!(
        row_ink(&h),
        painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == "BETA")
            .map(|g| g.color),
        "a hovered unstaged row must not read as the staged row beside it"
    );
}

/// Contract: a conflicted path is **no answer**, and no answer neither claims a
/// state nor says the words that would name one.
///
/// For an unmerged path `git diff HEAD` prints an ordinary two-sided diff whose
/// added lines *are* the conflict markers, `git diff` prints `diff --cc`, and
/// `git diff --cached` prints `* Unmerged path` with no section at all. So nothing
/// derivable from the patch the rows were built from may drive staged-ness here: it
/// would report a conflict's own `>>>>>>>` half as a staged addition. The per-line
/// oracle answers `None`, and `None` keeps the row's full treatment (muting it would
/// assert it is unstaged, which is the same lie in the other direction) while both
/// sets of words refuse: no hunk suffix, and an accessibility label that says the
/// state was not answered.
#[test]
fn a_conflicted_file_claims_no_staged_ness_anywhere() {
    let (_tmp, repo) = repo_conflicted();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("conf.txt"));

    assert!(
        painted_text(&h).iter().any(|t| t.starts_with("@@")),
        "the fixture's premise: the pane is rendering the conflicted file's rows, \
         got:\n{}",
        painted_text(&h).join("\n")
    );

    let unknown: Vec<_> = h
        .query_all_by_label_contains("(staged-ness unknown)")
        .collect();
    assert!(
        !unknown.is_empty(),
        "every changed row of a conflict says its state was not answered"
    );
    for claim in ["(staged)", "(unstaged)"] {
        assert!(
            h.query_all_by_label_contains(claim).next().is_none(),
            "a conflict's rows must not be labelled {claim}: the pane would be \
             guessing at the index from a patch that cannot support the guess"
        );
    }

    let headers: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("· "))
        .map(|g| g.text)
        .collect();
    assert!(
        headers.is_empty(),
        "no hunk of a conflicted file may name itself staged, part or not staged, \
         got {headers:?}"
    );

    let quiet: Vec<Color32> = filled_rects(&h)
        .into_iter()
        .map(|(_, c)| c)
        .filter(|c| *c == Palette::DIFF_ADD_UNSTAGED_BG || *c == Palette::DIFF_DEL_UNSTAGED_BG)
        .collect();
    assert!(
        quiet.is_empty(),
        "and nothing in it is drawn muted either: no answer is not `unstaged`"
    );
}

/// Contract: a row's accessibility label follows the index, and not the pixels.
///
/// The label is the only staged-ness channel that is not colour, so a stale one is
/// the pane telling a screen reader something false about the index while telling a
/// sighted user something true. This click moves the index and moves nothing the
/// pane shows: the displayed `HEAD ↔ worktree` patch is byte-identical on both
/// sides of it, and both halves of the clicked pair stay on screen. What moves is
/// the oracle's answer, because every completed operation drops the root's
/// `hunk_stats` entry, so the next frame re-resolves staged-ness against the index
/// the click just wrote. A label that kept saying `unstaged` is invisible to every
/// colour census in this suite, which is why it is asserted here as a word.
#[test]
fn a_rows_staged_ness_label_flips_after_the_click_that_moves_the_index() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    // The bytes the pane renders, taken from git's own answer for the one
    // comparison the read is pinned to.
    let displayed = git(&repo, &["diff", "HEAD", "--", "file.txt"]);
    let words = |h: &Harness<'_, AppState>| -> Vec<String> {
        h.query_all_by_label_contains("stag")
            .filter_map(|n| n.accesskit_node().label())
            .collect()
    };
    let suffixes = |h: &Harness<'_, AppState>| -> Vec<String> {
        painted_galleys(h)
            .into_iter()
            .filter(|g| g.text.starts_with("· "))
            .map(|g| g.text)
            .collect()
    };
    assert!(
        words(&h).contains(&"GAMMA (unstaged)".to_string()),
        "the fixture's premise: the worktree's own line is labelled quiet, got {:?}",
        words(&h)
    );
    assert_eq!(
        suffixes(&h),
        ["· part".to_string()],
        "and the fixture's premise again: git's two edits sit inside one hunk's \
         context, so its header already names the mixed case"
    );

    h.get_by_label("GAMMA (unstaged)").click();

    assert!(
        wait_until(15_000, || {
            h.step();
            words(&h).contains(&"GAMMA (staged)".to_string())
        }),
        "the clicked row's label must name the state the index now holds, got {:?}",
        words(&h)
    );
    assert!(
        !words(&h).contains(&"GAMMA (unstaged)".to_string()),
        "and the quiet word is gone rather than shadowed by a second node, got {:?}",
        words(&h)
    );
    // A stage is one ordinal, so the clicked addition's own deletion keeps the
    // word it had: the label states the row's state and never the consequence of
    // the click, which is the only wording that stays true when a row's click
    // reaches a pair, a whole file, or one line.
    assert!(
        words(&h).contains(&"gamma (unstaged)".to_string()),
        "the partner row of a staged line is still its own answer, got {:?}",
        words(&h)
    );
    // And the header word tracks the same fold: one line staged out of a hunk
    // holding four changed rows leaves the hunk mixed, so it must still say so
    // rather than claim `staged` for a row the index never took.
    assert_eq!(
        suffixes(&h),
        ["· part".to_string()],
        "the clicked hunk still holds both states, and its header must name that"
    );

    // git, independently of the pane: the word flipped because the index moved.
    assert!(
        git(&repo, &["diff", "--cached", "--", "file.txt"]).contains("+GAMMA"),
        "the label claims the line is staged, so git must say so too"
    );
    assert_eq!(
        git(&repo, &["diff", "HEAD", "--", "file.txt"]),
        displayed,
        "while the patch the pane renders did not move by one byte: the click \
         changed what is staged, never what is compared"
    );
    for row in ["GAMMA", "gamma"] {
        assert_painted(&h, row);
    }

    // The word and the paint answer from one resolution, so they cannot disagree
    // about which row the click reached.
    let at = galley_origin(&h, "GAMMA").expect("the clicked row still paints");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(at)),
        "the row the label now calls staged wears the staged band under it"
    );
}
