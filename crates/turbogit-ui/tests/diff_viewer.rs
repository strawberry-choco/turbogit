use std::os::unix::fs::PermissionsExt;
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

fn write_file(repo: &Path, name: &str, content: &str) {
    std::fs::write(repo.join(name), content).expect("writing file");
}

fn init_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    (tmp, repo)
}

fn repo_mixed() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(
        &repo,
        "file.txt",
        "alpha\nbeta\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    );
    git(&repo, &["add", "file.txt"]);
    write_file(
        &repo,
        "file.txt",
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\nGAMMA\niota\n",
    );
    (tmp, repo)
}

fn repo_staged_only() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "one.txt", "one\ntwo\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "one.txt", "one\nTWO\n");
    git(&repo, &["add", "one.txt"]);
    (tmp, repo)
}

fn repo_whitespace() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "ws.txt", "alpha\nbeta\ngamma\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "ws.txt", "alpha\nbeta\ng amma\n");
    (tmp, repo)
}

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

fn repo_two_file_commit() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    write_file(&repo, "one.txt", "alpha\nbeta\n");
    write_file(&repo, "two.txt", "gamma\ndelta\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "one.txt", "alpha\nBETA\n");
    write_file(&repo, "two.txt", "gamma\nDELTA\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c2"]);
    (tmp, repo)
}

/// A commit that `git` reports as a **rename with content edits**, similarity
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

/// Run `git <args>` in `dir` without asserting success, a merge that
/// conflicts exits non-zero, which is exactly the state under test. Kept local
/// because it is the opposite contract from `git_seed::git`.
fn git_expect_failure(dir: &Path, args: &[&str]) {
    let _ = Command::new("git").args(args).current_dir(dir).output();
}

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

fn diff_harness(repo: &Path) -> Harness<'static, AppState> {
    let state = AppState::new(repo.to_path_buf());
    assert!(
        !state.multi.roots.is_empty(),
        "repo root must be discovered"
    );
    shell_harness_over(state, egui::vec2(1008.0, 900.0))
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
    assert_painted(&harness, "Diff options");
    assert_painted(&harness, "Side-by-Side");
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
    assert_painted(&harness, "Diff options");
}

/// Step frames until painted output AND the diff read's activity stabilize
/// (the fingerprint includes the read's verdict so a late `DiffReady` can never
/// be mistaken for a settled frame). Budgeted by wall-clock time, not frame
/// count, so a contended `git` subprocess cannot starve it.
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

fn open_preview(harness: &mut Harness<'_, AppState>, root: &Path, file: &Path) {
    harness.state_mut().ui.preview_change = Some(ChangeRef::new(
        RootId(root.to_path_buf().into()),
        file.to_path_buf(),
    ));
    settle(harness);
}

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

#[test]
fn segmented_control_toggles_side_by_side_and_unified() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    assert!(!h.state().ui.diff_side_by_side);
    assert_not_painted(&h, "After Working tree");

    h.get_by_label("Side-by-Side").click();
    settle(&mut h);
    assert!(h.state().ui.diff_side_by_side);
    assert_painted(&h, "Before HEAD");
    assert_painted(&h, "After Working tree");

    h.get_by_label("Unified").click();
    settle(&mut h);
    assert!(!h.state().ui.diff_side_by_side);
    assert_not_painted(&h, "After Working tree");
}

#[test]
fn the_toolbar_leads_with_mode_and_options_and_closes_with_the_counts_and_nav() {
    let (_tmp, repo) = repo_two_hunks();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("nav.txt"));
    settle(&mut h);

    let seg = h.get_by_label("Side-by-Side").rect();
    let options = h.get_by_label("Diff options").rect();
    let prev = h.get_by_label("Previous hunk").rect();
    let next = h.get_by_label("Next hunk").rect();

    let seg_y = seg.center().y;
    for (label, rect) in [
        ("Diff options", options),
        ("Previous hunk", prev),
        ("Next hunk", next),
    ] {
        assert!(
            (rect.center().y - seg_y).abs() < 4.0,
            "`{label}` shares the toolbar's one row (centre {} vs mode toggle \
             {seg_y})",
            rect.center().y
        );
    }

    assert!(
        options.left() > seg.right(),
        "Diff options follows the mode toggle on the toolbar's leading edge"
    );
    assert!(
        prev.left() > options.right(),
        "the counts and hunk nav close the toolbar, right of Diff options"
    );

    assert!(
        next.right() > options.right() + 100.0,
        "the hunk nav is right-aligned to the toolbar's trailing edge \
         (nav right {:.1}, Diff options right {:.1})",
        next.right(),
        options.right()
    );
}

#[test]
fn hunk_navigation_counts_and_steps_across_two_hunks() {
    let (_tmp, repo) = repo_two_hunks();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("nav.txt"));

    assert_painted(&h, "1/2");
    assert_eq!(h.state().ui.diff_current_hunk, 0);

    h.get_by_label("Next hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 1);
    assert_painted(&h, "2/2");

    h.get_by_label("Next hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 1);

    h.get_by_label("Previous hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 0);
    assert_painted(&h, "1/2");

    h.get_by_label("Previous hunk").click();
    settle(&mut h);
    assert_eq!(h.state().ui.diff_current_hunk, 0);
}

#[test]
fn whitespace_policy_stops_painting_the_line_pair_it_calls_equal() {
    use turbogit_domain::model::DiffWhitespace;
    let (_tmp, repo) = repo_whitespace();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("ws.txt"));

    assert_painted(&h, "Diff options");
    assert_eq!(h.state().ui.diff_policies.whitespace, DiffWhitespace::None);
    assert_painted(&h, "gamma");
    assert_painted(&h, "g amma");

    h.state_mut().ui.diff_policies.whitespace = DiffWhitespace::IgnoreWhitespace;
    settle(&mut h);
    assert_not_painted(&h, "gamma");
    assert_painted(&h, "g amma");

    h.state_mut().ui.diff_policies.whitespace = DiffWhitespace::TrimWhitespace;
    settle(&mut h);
    assert_painted(&h, "gamma");

    h.state_mut().ui.diff_policies.whitespace = DiffWhitespace::None;
    settle(&mut h);
    assert_painted(&h, "gamma");
    assert_painted(&h, "g amma");
}

#[test]
fn add_del_rows_paint_token_backgrounds_with_muted_gutters() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

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

    assert!(
        galley_origin(&h, "8").is_some(),
        "gutter line numbers must be painted"
    );

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

#[test]
fn ghost_icon_controls_keep_their_labels_and_enabled_flags() {
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
    for label in ["Stage hunk 1", "Unstage hunk 1"] {
        assert!(
            !h.get_by_label(label).accesskit_node().is_disabled(),
            "the `{label}` gutter control must stay findable by label and \
             enabled on a clean hunk"
        );
    }

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

fn diff_hunk_headers(repo: &Path) -> Vec<String> {
    git(repo, &["diff", "-U3", "--", "deep.txt"])
        .lines()
        .filter(|l| l.starts_with("@@"))
        .map(str::to_owned)
        .collect()
}

fn painted_hunk_headers(harness: &Harness<'_, AppState>) -> Vec<String> {
    painted_text(harness)
        .into_iter()
        .filter(|t| t.starts_with("@@"))
        .collect()
}

/// Settle a frame that ISSUED a scroll. The aim is applied when the scroll area
/// ends the frame it was issued in, so the new offset first appears in the NEXT
/// frame's paint, and `settle` returns as soon as two frames agree, which it
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
    settle(&mut h);
    open_preview(&mut h, repo, &repo.join("deep.txt"));
    assert_eq!(h.state().ui.diff_side_by_side, side_by_side);
    assert!(
        painted_hunk_headers(&h).len() < HUNKS,
        "the pane cannot show all {HUNKS} headers at once — the premise of this test"
    );

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

#[test]
fn hunk_navigation_aims_the_hunk_it_names_side_by_side() {
    let (_tmp, repo) = repo_deep_diff(20, 15, 20);
    hunk_aim_sequence(&repo, true);
}

#[test]
fn hunk_navigation_aims_the_hunk_it_names_unified() {
    let (_tmp, repo) = repo_deep_diff(20, 15, 20);
    hunk_aim_sequence(&repo, false);
}

/// Half a pixel of slack: egui rounds UI rects, so exact float equality is the
/// wrong question to ask about two rects that are meant to coincide.
const EPS: f32 = 0.5;

fn axis_span(harness: &Harness<'_, AppState>) -> Rect {
    let mode = harness.get_by_label("Unified").rect();
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
    harness.set_size(egui::vec2(1008.0, 900.0));
    harness
}

#[track_caller]
fn assert_no_comparison_chips(harness: &Harness<'_, AppState>, surface: &str) {
    for label in ["Repo", "Staged", "Local"] {
        assert!(
            harness.query_by_label(label).is_none(),
            "`{label}` was an arm of the deleted axis, so {surface} must not offer it"
        );
    }
}

#[test]
fn the_removed_axis_leaves_no_group_anywhere() {
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

fn hovered_file_row(harness: &mut Harness<'_, AppState>) -> (Rect, Pos2) {
    let galley = painted_galleys(harness)
        .into_iter()
        .find(|g| g.text.starts_with("diff --git "))
        .expect("the file row's own line is painted");
    let point = Pos2::new(galley.pos.x + 1.0, galley.rect.center().y);
    harness.hover_at(point);
    harness.run();
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

#[test]
fn a_single_file_pane_shows_no_plumbing_and_counts_on_the_toolbar() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

    let plumbing: Vec<_> = painted_galleys(&h)
        .into_iter()
        .filter(|g| {
            [
                "diff --git ",
                "index ",
                "--- ",
                "+++ ",
                "new file mode ",
                "old mode ",
                "new mode ",
            ]
            .iter()
            .any(|prefix| g.text.starts_with(prefix))
        })
        .collect();
    assert!(
        plumbing.is_empty(),
        "a single-file pane paints no git plumbing: {plumbing:?}"
    );

    let row = toolbar_row(&h);
    for (text, ink) in [
        ("+2", Palette::DIFF_ADD_ACCENT),
        ("\u{2212}2", Palette::DIFF_DEL_ACCENT),
    ] {
        let on_toolbar: Vec<_> = painted_galleys(&h)
            .into_iter()
            .filter(|g| {
                g.text == text
                    && g.rect.center().y >= row.top() - EPS
                    && g.rect.center().y <= row.bottom() + EPS
            })
            .collect();
        assert_eq!(
            on_toolbar.len(),
            1,
            "`{text}` is the pane's one change-size statistic on the toolbar: \
             {on_toolbar:?}"
        );
        assert_eq!(on_toolbar[0].color, ink, "`{text}` keeps its stats accent");
        assert!(
            on_toolbar[0].rect.left() > row.right(),
            "`{text}` sits right of the mode control: {:?} against {row:?}",
            on_toolbar[0].rect
        );
    }

    let toggles: Vec<Rect> = h
        .get_all_by_label("Diff options")
        .map(|n| n.rect())
        .collect();
    assert_eq!(toggles.len(), 1, "exactly one trigger paints");
    assert!(
        toggles[0].center().y >= row.top() - EPS && toggles[0].center().y <= row.bottom() + EPS,
        "the policy menu belongs on the toolbar's row: {:?} against {row:?}",
        toggles[0]
    );
    assert!(
        toggles[0].left() > row.right(),
        "the trigger sits right of the mode control, the row's trailing end: \
         {:?} against {row:?}",
        toggles[0]
    );
}

#[test]
fn multi_file_sections_keep_their_counts_on_the_file_rows() {
    let (_tmp, repo) = repo_two_file_commit();
    let mut h = commit_diff_harness(&repo);
    settle(&mut h);

    let file_labels: Vec<_> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("diff --git "))
        .collect();
    assert_eq!(
        file_labels.len(),
        2,
        "both sections keep their opening line: {:?}",
        file_labels
            .iter()
            .map(|g| g.text.as_str())
            .collect::<Vec<_>>()
    );

    let row = toolbar_row(&h);
    for stat in ["+1", "\u{2212}1"] {
        let galleys: Vec<_> = painted_galleys(&h)
            .into_iter()
            .filter(|g| g.text == stat)
            .collect();
        assert_eq!(
            galleys.len(),
            2,
            "one `{stat}` per section, both on their file rows: {galleys:?}"
        );
        for label in &file_labels {
            let on_row = galleys
                .iter()
                .find(|g| (g.rect.center().y - label.rect.center().y).abs() <= EPS);
            assert!(
                on_row.is_some(),
                "the section opening at {:?} carries its own `{stat}`: {galleys:?}",
                label.text
            );
            assert!(
                !row.contains(on_row.unwrap().rect.center()),
                "the toolbar paints no per-file statistic: {stat:?} at {:?}",
                on_row.unwrap().rect
            );
        }
    }
}

#[test]
fn a_mode_only_change_states_its_empty_scroller() {
    let (_tmp, repo) = init_repo();
    let script = repo.join("script.sh");
    write_file(&repo, "script.sh", "echo hi\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("chmod the worktree file");
    assert!(
        git(&repo, &["diff", "HEAD"]).contains("new mode 100755"),
        "the seeded change must be mode-only, got:\n{}",
        git(&repo, &["diff", "HEAD"])
    );

    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &script);
    settle(&mut h);

    assert_painted(&h, "No content changes.");
}

fn toolbar_row(harness: &Harness<'_, AppState>) -> Rect {
    let mode = harness.get_by_label("Unified").rect();
    filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::SURFACE_2 && r.contains(mode.center()))
        .map(|(r, _)| r)
        .expect("the view-mode control paints its own track")
}

#[test]
fn the_file_row_takes_its_geometry_from_the_row_shell() {
    let (_tmp, repo) = repo_two_file_commit();
    let mut h = commit_diff_harness(&repo);
    settle(&mut h);

    let (band, _) = hovered_file_row(&mut h);
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

#[test]
fn the_file_rows_occupants_do_not_overlap_the_rows_own_click_targets() {
    let (_tmp, repo) = repo_two_file_commit();
    let mut h = commit_diff_harness(&repo);
    settle(&mut h);

    let (band, _) = hovered_file_row(&mut h);
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
        "each section's one pair is on screen, every row on its own band: {changed:?}"
    );
    for row in &changed {
        assert!(
            !row.intersects(band),
            "the file row's own band overlaps a changed row's band {row:?}: {band:?}"
        );
    }
}

const DIFF_ACTIONS_SRC: &str = include_str!("../src/ui/diff/actions.rs");

#[test]
fn the_diff_surface_declares_no_change_status_resolver_of_its_own() {
    assert!(
        !DIFF_ACTIONS_SRC.contains("fn preview_status"),
        "ui::diff::actions must not spell the change-status resolver: \
         turbogit_app::granular::preview_status owns it (lane 5, ticket 12), \
         and both diff frames call it"
    );
}

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

#[test]
fn hover_stays_visible_on_a_muted_row() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

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

#[test]
fn a_rows_staged_ness_label_flips_after_the_click_that_moves_the_index() {
    let (_tmp, repo) = repo_mixed();
    let mut h = diff_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("file.txt"));

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
    assert!(
        words(&h).contains(&"gamma (unstaged)".to_string()),
        "the partner row of a staged line is still its own answer, got {:?}",
        words(&h)
    );
    assert_eq!(
        suffixes(&h),
        ["· part".to_string()],
        "the clicked hunk still holds both states, and its header must name that"
    );

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

    let at = galley_origin(&h, "GAMMA").expect("the clicked row still paints");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::DIFF_ADD_BG && r.contains(at)),
        "the row the label now calls staged wears the staged band under it"
    );
}

/// 60 lines with one line split in two at 15 and another replaced at 45. Git
/// answers `-U3` with `@@ -12,7 +12,8 @@` and `@@ -42,7 +43,7 @@` — measured, not
/// derived — so the patch leaves three runs unprinted: 11 lines above the first
/// hunk, 23 between the hunks, 12 below the last. The split pushes the two sides
/// apart, so the middle run numbers 19-41 old against 20-42 new.
fn repo_context_gaps() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=60).map(|i| format!("l{i:02}")).collect();
    write_file(&repo, "wide.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[14] = "X15a".to_string();
    edited.insert(15, "X15b".to_string());
    edited[45] = "X45".to_string();
    write_file(&repo, "wide.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

/// Tall enough that the whole of the fixture's diff is on screen at once: a
/// reveal that opens 23 lines must not have to scroll to be checked.
fn gap_harness(repo: &Path) -> Harness<'static, AppState> {
    let state = AppState::new(repo.to_path_buf());
    shell_harness_over(state, egui::vec2(1008.0, 1800.0))
}

fn open_gaps(h: &mut Harness<'static, AppState>, repo: &Path) {
    settle(h);
    open_preview(h, repo, &repo.join("wide.txt"));
    let headers: Vec<String> = painted_hunk_headers(h);
    assert_eq!(
        headers,
        ["@@ -12,7 +12,8 @@ l11", "@@ -42,7 +43,7 @@ l41"],
        "the fixture's premise: both hunks' headers are painted, so both gaps are \
         on screen"
    );
}

/// A real press-and-release at `pos`, the way egui sees a mouse click.
fn click_at(harness: &mut Harness<'_, AppState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.step();
    harness.drag_at(pos);
    harness.step();
    harness.drop_at(pos);
    settle(harness);
}

/// A revealed line and its gutter number must be on the same row: a line opened
/// with the wrong base paints the right text in the wrong place.
#[track_caller]
fn assert_row_numbered(h: &Harness<'_, AppState>, text: &str, number: &str) {
    let at = galley_origin(h, text).unwrap_or_else(|| panic!("`{text}` painted"));
    let num =
        galley_origin(h, number).unwrap_or_else(|| panic!("gutter number `{number}` painted"));
    assert!(
        (at.y - num.y).abs() < 4.0 && num.x < at.x,
        "`{text}` sits at {at:?} while its gutter `{number}` is at {num:?}"
    );
}

/// The gap bars on screen, each with its button's row, top row first. `Expand
/// log` is the log window's own control, so the family is named by its three
/// verbs rather than by the word `Expand`.
fn gap_bars(h: &Harness<'_, AppState>) -> Vec<(String, Rect)> {
    let mut bars: Vec<(String, Rect)> = h
        .root()
        .children_recursive()
        .filter_map(|node| {
            let label = node.accesskit_node().label()?;
            let offered = [
                "Expand next lines",
                "Expand previous lines",
                "Expand lines between",
            ]
            .iter()
            .any(|start| label.starts_with(start));
            offered.then(|| (label, node.rect()))
        })
        .collect();
    bars.sort_by(|a, b| a.1.center().y.total_cmp(&b.1.center().y));
    bars
}

/// The labels of the gap bars on screen, top row first.
fn bar_labels(h: &Harness<'_, AppState>) -> Vec<String> {
    gap_bars(h).into_iter().map(|(label, _)| label).collect()
}

/// The gutter numbers painted strictly between two rows, in the order their rows
/// paint. A revealed line's number is what a wrong block order corrupts first.
#[track_caller]
fn numbers_between(h: &Harness<'_, AppState>, from_y: f32, to_y: f32) -> Vec<usize> {
    let mut rows: Vec<(f32, usize)> = painted_galleys(h)
        .into_iter()
        .filter(|g| !g.text.is_empty() && g.text.chars().all(|c| c.is_ascii_digit()))
        .map(|g| {
            (
                g.rect.center().y,
                g.text.parse::<usize>().expect("a gutter of digits"),
            )
        })
        .filter(|(y, _)| *y > from_y + 4.0 && *y < to_y - 4.0)
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.dedup_by(|a, b| a.0 == b.0);
    rows.into_iter().map(|(_, number)| number).collect()
}

#[test]
fn a_bar_names_what_its_gap_hides_and_a_tap_opens_the_lines_it_promised() {
    let (_tmp, repo) = repo_context_gaps();
    let mut h = gap_harness(&repo);
    open_gaps(&mut h, &repo);

    for caption in ["11 unchanged lines hidden", "23 unchanged lines hidden"] {
        assert_painted(&h, caption);
    }
    assert_eq!(
        bar_labels(&h),
        [
            "Expand previous lines at the top of the file",
            "Expand next lines below hunk 1",
            "Expand previous lines above hunk 2",
            "Expand next lines at the end of the file",
        ],
        "one bar per affordance, top row first; painted:\n{}",
        painted_text(&h).join("\n")
    );
    assert_not_painted(&h, "l32");

    h.get_by_label("Expand previous lines above hunk 2").click();
    settle(&mut h);

    for offset in 0..10u32 {
        let line = 32 + offset;
        assert_row_numbered(&h, &format!("l{line:02}"), &(line + 1).to_string());
    }
    assert_not_painted(&h, "l31");
    assert_painted(&h, "13 unchanged lines hidden");
}

/// The centre of the row a painted string sits on, so row distance can be
/// measured rather than assumed from a constant the test would rather not own.
#[track_caller]
fn row_centre(h: &Harness<'_, AppState>, text: &str) -> f32 {
    painted_galleys(h)
        .into_iter()
        .find(|galley| galley.text == text)
        .unwrap_or_else(|| panic!("`{text}` painted"))
        .rect
        .center()
        .y
}

#[test]
fn a_long_gap_offers_one_single_button_bar_at_each_of_its_edges() {
    let (_tmp, repo) = repo_context_gaps();
    let mut h = gap_harness(&repo);
    open_gaps(&mut h, &repo);

    let interior: Vec<(String, Rect)> = gap_bars(&h)
        .into_iter()
        .filter(|(label, _)| label.contains("hunk"))
        .collect();
    // Two bars that share a widget id publish one accessibility node, so this set
    // is also what pins the bars to different id verbs.
    assert_eq!(
        interior
            .iter()
            .map(|(label, _)| label.as_str())
            .collect::<Vec<_>>(),
        [
            "Expand next lines below hunk 1",
            "Expand previous lines above hunk 2",
        ],
        "the run between the hunks is offered twice, once per edge, and its two bars \
         name different hunks"
    );
    let next = interior[0].1;
    let previous = interior[1].1;
    let mut centres: Vec<f32> = gap_bars(&h)
        .into_iter()
        .map(|(_, rect)| rect.center().y)
        .collect();
    centres.dedup_by(|a, b| (*a - *b).abs() < 4.0);
    assert_eq!(
        centres.len(),
        4,
        "one button per bar: no row of this pane carries two expand buttons"
    );

    let pitch = row_centre(&h, "l18") - row_centre(&h, "l17");
    let lower_header = "@@ -42,7 +43,7 @@ l41";
    let one_row_apart = |above: f32, below: f32| (above - below - pitch).abs() < pitch / 2.0;
    assert!(
        one_row_apart(next.center().y, row_centre(&h, "l18")),
        "the next bar is the row after hunk 1's last body line, not the strip at the \
         gap's top edge it replaced: {next:?}"
    );
    assert!(
        one_row_apart(previous.center().y, next.center().y),
        "the two bars are separate rows one pitch apart, the next one above: {next:?} \
         against {previous:?}"
    );
    assert!(
        one_row_apart(row_centre(&h, lower_header), previous.center().y),
        "the previous bar touches the block it owns, which is still empty, so it sits on \
         the row above the lower hunk's header: {previous:?}"
    );
}

#[test]
fn opening_a_gaps_both_edges_empties_it_and_the_strip_goes_with_it() {
    let (_tmp, repo) = repo_context_gaps();
    let mut h = gap_harness(&repo);
    open_gaps(&mut h, &repo);

    h.get_by_label("Expand previous lines above hunk 2").click();
    settle(&mut h);
    assert!(
        h.query_by_label("Expand previous lines above hunk 2")
            .is_some(),
        "ten of the run's 23 lines opened, so 13 are still hidden and both bars stay"
    );
    assert_painted(&h, "13 unchanged lines hidden");

    h.get_by_label("Expand next lines below hunk 1").click();
    settle(&mut h);
    assert_row_numbered(&h, "l19", "20");
    assert_row_numbered(&h, "l41", "42");
    assert_painted(&h, "3 unchanged lines hidden");
    assert_eq!(
        bar_labels(&h),
        [
            "Expand previous lines at the top of the file",
            "Expand lines between hunks 1 and 2",
            "Expand next lines at the end of the file",
        ],
        "three lines left is one step from empty, so the gap's two bars became one"
    );
    let collapsed = h.get_by_label("Expand lines between hunks 1 and 2").rect();
    assert_eq!(
        numbers_between(&h, row_centre(&h, "l18"), collapsed.center().y),
        (20..=29).collect::<Vec<_>>(),
        "the collapsed bar sits below the run it opened from the top edge"
    );
    assert_eq!(
        numbers_between(
            &h,
            collapsed.center().y,
            row_centre(&h, "@@ -42,7 +43,7 @@ l41")
        ),
        (33..=42).collect::<Vec<_>>(),
        "and above the run the suppressed bar handed it, so one ascending reading still \
         reaches the lower hunk while the three lines left hidden paint nothing"
    );

    h.get_by_label("Expand lines between hunks 1 and 2").click();
    settle(&mut h);
    for away in [
        "Expand lines between hunks 1 and 2",
        "Expand next lines below hunk 1",
        "Expand previous lines above hunk 2",
    ] {
        assert!(
            h.query_by_label(away).is_none(),
            "a gap with nothing left hidden offers no bar, and `{away}` still answers \
             to a label"
        );
    }
    assert_not_painted(&h, "23 unchanged lines hidden");
    assert_not_painted(&h, "13 unchanged lines hidden");
    for line in 19..=41 {
        assert_painted(&h, &format!("l{line:02}"));
    }
    for offset in 0..23u32 {
        assert_row_numbered(&h, &format!("l{}", 19 + offset), &(20 + offset).to_string());
    }
}

#[test]
fn a_revealed_line_carries_no_stage_target_and_a_click_on_it_moves_no_index() {
    let (_tmp, repo) = repo_context_gaps();
    let mut h = gap_harness(&repo);
    open_gaps(&mut h, &repo);
    h.get_by_label("Expand previous lines above hunk 2").click();
    settle(&mut h);

    for state in ["staged", "unstaged"] {
        assert!(
            h.query_by_label(&format!("l35 ({state})")).is_none(),
            "an opened line is context, so nothing may name it `{state}`"
        );
    }
    let at = galley_origin(&h, "l35").expect("the opened line painted");
    click_at(&mut h, egui::Pos2::new(at.x + 40.0, at.y + 4.0));

    assert_eq!(
        git(&repo, &["diff", "--cached", "--name-only"]).trim(),
        "",
        "clicking an opened line must not touch the index"
    );
    assert_row_numbered(&h, "l35", "36");
}

/// The number painted immediately left of `at` on its own row, which is the
/// gutter of the cell that holds `at`.
#[track_caller]
fn gutter_left_of(h: &Harness<'_, AppState>, at: Pos2) -> String {
    painted_galleys(h)
        .into_iter()
        .filter(|g| {
            (g.pos.y - at.y).abs() < 4.0
                && g.pos.x < at.x
                && !g.text.is_empty()
                && g.text.chars().all(|c| c.is_ascii_digit())
        })
        .max_by(|a, b| a.pos.x.total_cmp(&b.pos.x))
        .map(|g| g.text)
        .unwrap_or_else(|| panic!("no gutter number on the row at {at:?}"))
}

#[test]
fn side_by_side_opens_a_gap_as_one_row_of_two_cells_each_numbered_by_its_own_side() {
    let (_tmp, repo) = repo_context_gaps();
    let mut h = gap_harness(&repo);
    h.state_mut().ui.diff_side_by_side = true;
    open_gaps(&mut h, &repo);
    assert_not_painted(&h, "l32");

    h.get_by_label("Expand previous lines above hunk 2").click();
    settle(&mut h);

    let cells: Vec<Pos2> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text == "l32")
        .map(|g| g.pos)
        .collect();
    assert_eq!(
        cells.len(),
        2,
        "an opened line is one row whose two cells both hold it"
    );
    assert!(cells[0].x < cells[1].x, "the cells keep their order");
    assert_eq!(
        gutter_left_of(&h, cells[0]),
        "32",
        "the left cell numbers the old side"
    );
    assert_eq!(
        gutter_left_of(&h, cells[1]),
        "33",
        "the right cell numbers the new side, which the split at line 15 moved one \
         apart"
    );
}

/// 18 lines with one replacement at 15. Git prints `@@ -12,7 +12,7 @@`, so the
/// last hunk's trailing context reaches the patch's context radius and the run
/// below it is not provably empty from the patch — while the file, in fact, ends
/// at that hunk's last line.
fn repo_short_tail() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=18).map(|i| format!("t{i:02}")).collect();
    write_file(&repo, "tail.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[14] = "X15".to_string();
    write_file(&repo, "tail.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

#[test]
fn a_tail_that_holds_nothing_shows_its_strip_once_and_then_never_again() {
    let (_tmp, repo) = repo_short_tail();
    let mut h = gap_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("tail.txt"));
    assert_eq!(
        painted_hunk_headers(&h),
        ["@@ -12,7 +12,7 @@ t11"],
        "the fixture's premise: one hunk whose trailing context is a full radius"
    );
    assert_painted(&h, "Unchanged lines hidden");
    assert_not_painted(&h, "Reading the file…");
    let end = "Expand next lines at the end of the file";
    assert!(
        h.query_by_label(end).is_some(),
        "the bar exists before it can prove it is needed, because only the file \
         knows its own length"
    );
    assert_painted(&h, "t18");

    h.get_by_label(end).click();
    settle(&mut h);
    assert!(
        h.query_by_label(end).is_none(),
        "one tap answered 0 lines hidden, so the dead affordance is gone"
    );
    assert_not_painted(&h, "Unchanged lines hidden");
    assert_painted(&h, "t18");
}

#[test]
fn a_tail_reports_unknown_until_its_file_lands_and_a_number_after() {
    let (_tmp, repo) = repo_context_gaps();
    let mut h = gap_harness(&repo);
    open_gaps(&mut h, &repo);
    assert_painted(&h, "Unchanged lines hidden");
    assert_not_painted(&h, "l49");

    let end = "Expand next lines at the end of the file";
    h.get_by_label(end).click();
    settle(&mut h);
    for offset in 0..10u32 {
        assert_row_numbered(&h, &format!("l{}", 49 + offset), &(50 + offset).to_string());
    }
    assert_painted(&h, "2 unchanged lines hidden");
    assert!(
        h.query_by_label(end).is_some(),
        "a run at a file's end keeps its one bar however little it has left to open"
    );

    h.get_by_label(end).click();
    settle(&mut h);
    assert_row_numbered(&h, "l59", "60");
    assert_row_numbered(&h, "l60", "61");
    assert!(
        h.query_by_label(end).is_none(),
        "the run to the end of the file is open, so `{end}` is gone"
    );
    assert_not_painted(&h, "Unchanged lines hidden");
}

/// Two committed files of 20 lines each, one replacement at line 12 in both, so
/// every section carries its own leading run of 8 and its own tail of 5.
fn repo_two_file_gaps() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let make = |c: char, n: &str| -> String {
        format!(
            "{}\n",
            (1..=20)
                .map(|i| format!("{c}{i:02}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
        .replace(&format!("{c}12"), n)
    };
    write_file(&repo, "one.txt", &make('a', "alpha"));
    write_file(&repo, "two.txt", &make('b', "bravo"));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    write_file(&repo, "one.txt", &make('a', "ALPHA"));
    write_file(&repo, "two.txt", &make('b', "BRAVO"));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c2"]);
    (tmp, repo)
}

#[test]
fn every_section_of_a_multi_file_patch_carries_its_own_strips() {
    fn tails(h: &Harness<'_, AppState>) -> Vec<Rect> {
        let mut rows: Vec<Rect> = h
            .query_all_by_label("Expand next lines at the end of the file")
            .map(|node| node.rect())
            .collect();
        rows.sort_by(|a, b| a.center().y.total_cmp(&b.center().y));
        rows
    }

    let (_tmp, repo) = repo_two_file_gaps();
    let mut h = commit_diff_harness(&repo);
    // Tall enough that both sections' bars and the five opened rows all fit: a
    // clipped row is absent from the tree for a reason that is not the feature.
    h.set_size(egui::vec2(1008.0, 1800.0));
    settle(&mut h);
    let captions = |h: &Harness<'_, AppState>, words: &str| {
        painted_text(h).into_iter().filter(|t| t == words).count()
    };
    assert_eq!(
        captions(&h, "8 unchanged lines hidden"),
        2,
        "one leading bar per section, painted:\n{}",
        painted_text(&h).join("\n")
    );
    assert_eq!(
        captions(&h, "Unchanged lines hidden"),
        2,
        "two tails, each sized only by its own file's text, and neither has been asked yet"
    );
    assert_not_painted(&h, "a16");
    assert_not_painted(&h, "b16");
    // A bar at a file's edge is named for the file rather than for a hunk, so both
    // sections answer to the same words and the one to press is found by its row.
    assert_eq!(tails(&h).len(), 2, "one tail bar per section");

    let first_tail = tails(&h)[0];
    click_at(&mut h, first_tail.center());

    for line in 16..=20 {
        assert_painted(&h, &format!("a{line}"));
    }
    assert_not_painted(&h, "b16");
    assert_eq!(
        captions(&h, "8 unchanged lines hidden"),
        2,
        "the other section's bars are its own"
    );
    assert_eq!(
        tails(&h).len(),
        1,
        "one section's tail opened to the end of its file, and the other is still closed"
    );
    assert_eq!(
        h.query_all_by_label("Expand previous lines at the top of the file")
            .count(),
        2,
        "neither leading bar moved when its neighbour's tail opened"
    );
}

/// 30 lines with one replacement at 8 and another at 23. Git answers `-U3` with
/// `@@ -5,7 +5,7 @@` and `@@ -20,7 +20,7 @@` — measured, not derived — so the run
/// between the hunks is 8 lines: less than one reveal step, which is what makes
/// this gap collapse to a single bar.
fn repo_short_interior_gap() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=30).map(|i| format!("t{i:02}")).collect();
    write_file(&repo, "tight.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[7] = "E8".to_string();
    edited[22] = "E23".to_string();
    write_file(&repo, "tight.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

#[test]
fn a_short_interior_gap_offers_one_bar_that_finishes_it_in_one_tap() {
    let (_tmp, repo) = repo_short_interior_gap();
    let mut h = gap_harness(&repo);
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("tight.txt"));
    assert_eq!(
        painted_hunk_headers(&h),
        ["@@ -5,7 +5,7 @@ t04", "@@ -20,7 +20,7 @@ t19"],
        "the fixture's premise: a run of 8 lines between the two hunks"
    );
    assert_eq!(
        bar_labels(&h),
        [
            "Expand previous lines at the top of the file",
            "Expand lines between hunks 1 and 2",
            "Expand next lines at the end of the file",
        ],
        "a gap one step from empty names its whole run instead of an edge, and offers \
         it once"
    );
    assert_painted(&h, "8 unchanged lines hidden");
    assert_not_painted(&h, "t12");

    h.get_by_label("Expand lines between hunks 1 and 2").click();
    settle(&mut h);

    assert!(
        h.query_by_label("Expand lines between hunks 1 and 2")
            .is_none(),
        "one tap opened all 8 lines, so the bar is gone"
    );
    assert_not_painted(&h, "8 unchanged lines hidden");
    for line in 12..=19 {
        assert_row_numbered(&h, &format!("t{line:02}"), &line.to_string());
    }
}

/// 100 lines with one replacement at 8 and another at 90. Git answers `-U3` with
/// `@@ -5,7 +5,7 @@` and `@@ -87,7 +87,7 @@`, so the run between them is 75 lines:
/// long enough that both bars can own a block at once and a previous bar still has
/// lines left to open after two taps.
fn repo_long_interior_gap() -> (tempfile::TempDir, PathBuf) {
    let (tmp, repo) = init_repo();
    let base: Vec<String> = (1..=100).map(|i| format!("g{i:02}")).collect();
    write_file(&repo, "tall.txt", &format!("{}\n", base.join("\n")));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "c1"]);
    let mut edited = base;
    edited[7] = "E8".to_string();
    edited[89] = "E90".to_string();
    write_file(&repo, "tall.txt", &format!("{}\n", edited.join("\n")));
    (tmp, repo)
}

#[test]
fn a_previous_bar_keeps_its_row_and_its_lines_keep_reading_into_the_hunk_below() {
    let (_tmp, repo) = repo_long_interior_gap();
    // Taller than the other gap fixtures: this test ends with 40 opened rows in one
    // gap, and a row scrolled out of the pane is missing from the tree for a reason
    // that is not the feature.
    let mut h = shell_harness_over(
        AppState::new(repo.to_path_buf()),
        egui::vec2(1008.0, 2200.0),
    );
    settle(&mut h);
    open_preview(&mut h, &repo, &repo.join("tall.txt"));
    assert_eq!(
        painted_hunk_headers(&h),
        ["@@ -5,7 +5,7 @@ g04", "@@ -87,7 +87,7 @@ g86"],
        "the fixture's premise: a run of 75 lines between the two hunks"
    );

    let header_text = "@@ -87,7 +87,7 @@ g86";
    let previous = "Expand previous lines above hunk 2";

    h.get_by_label("Expand next lines below hunk 1").click();
    settle(&mut h);
    let next = h.get_by_label("Expand next lines below hunk 1").rect();
    let placed = h.get_by_label(previous).rect();
    let header = row_centre(&h, header_text);
    assert_eq!(
        numbers_between(&h, row_centre(&h, "g11"), next.center().y),
        (12..=21).collect::<Vec<_>>(),
        "the next bar sits at the frontier of the run it opened, so the ten lines are \
         above it and not below it"
    );
    assert_eq!(
        numbers_between(&h, next.center().y, placed.center().y),
        Vec::<usize>::new(),
        "the still-hidden middle paints nothing, so the two bars are consecutive rows"
    );
    assert_eq!(
        numbers_between(&h, placed.center().y, header),
        Vec::<usize>::new(),
        "the previous bar sits between that block and the lower hunk's header, with \
         nothing of its own open yet"
    );

    h.get_by_label(previous).click();
    settle(&mut h);
    let first = h.get_by_label(previous).rect();
    assert!(
        (first.center().y - placed.center().y).abs() < 1.0,
        "the bar that was tapped stayed on its own row while its block grew: {placed:?} \
         against {first:?}"
    );
    assert_eq!(
        numbers_between(&h, placed.center().y, row_centre(&h, header_text)),
        (77..=86).collect::<Vec<_>>(),
        "ten lines opened from the bottom edge read ascending into the lower hunk"
    );

    h.get_by_label(previous).click();
    settle(&mut h);
    assert!(
        (h.get_by_label(previous).rect().center().y - placed.center().y).abs() < 1.0,
        "and the second tap moved it no further"
    );
    assert_eq!(
        numbers_between(&h, placed.center().y, row_centre(&h, header_text)),
        (57..=86).collect::<Vec<_>>(),
        "the second tap put its twenty lines *above* the block it already had, so the \
         run still ends on the line the lower hunk's header follows"
    );
}
