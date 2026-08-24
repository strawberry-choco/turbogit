//! Issue #22 — Feedback chrome: toasts, confirm prompts, VCS popup, palette.
//!
//! Headless egui_kittest harness (same pattern as `redesign_harness.rs`)
//! driving [`turbogit::ui::render`] over a **real temp git repository**.
//! Asserts only on public surfaces: painted shapes (text galleys, filled
//! rects, stroked paths) and public `AppState` transitions.
//!
//! Covered (spec §7.1/§10, R4.x, ADR-0011):
//! - each toast kind paints its semantic STATE_* token as accent bar + icon
//! - Dismiss clears the toast
//! - confirm cycle still executes (OK) and cancels (Cancel) destructive ops
//! - VCS operations popup renders popup chrome with its EXACT action set
//! - command palette retains every prior entry and gains Go to Log /
//!   Open Welcome / Toggle Toolbar, each verified reachable

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{assert_not_painted, assert_painted, filled_rects, galley_origin, painted_text};
use egui::epaint::ColorMode;
use egui::{Color32, Pos2, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use turbogit::state::{AppState, PendingConfirm, Tab, Toast, ToastKind};
use turbogit::theme::Palette;
use turbogit::ui::popups::Action;

// --- git fixture ---------------------------------------------------------------

/// Run `git <args>` in `dir`, panicking on failure. Identity comes from env so
/// no per-repo config calls are needed.
fn git(dir: &Path, args: &[&str]) {
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
}

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "init"]);
}

/// A project dir holding one real repo `alpha` on `main` with local branches
/// `feature-a` and `feature-b` (disposable subjects for the confirm cycle).
fn repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    let alpha = project.join("alpha");
    commit_readme(&alpha);
    git(&alpha, &["branch", "feature-a"]);
    git(&alpha, &["branch", "feature-b"]);
    (tmp, project)
}

// --- harness ---------------------------------------------------------------------

/// Harness over a real repo-backed project. Setup mirrors production app.rs:
/// event pump first, then dark-only tokens + embedded fonts once, then render.
fn feedback_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

/// Step frames until painted output is stable for 3 consecutive frames
/// (tolerates slow background git subprocesses).
fn settle_quiet(harness: &mut Harness<'_, AppState>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        let fp = format!("{:?}", painted_text(harness));
        if fp == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = fp;
        }
    }
    panic!("feedback layout did not settle within 300 frames");
}

/// Step frames until `pred` holds on public state (async op completion).
fn pump_until(harness: &mut Harness<'_, AppState>, what: &str, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(harness.state()) {
            return;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let s = harness.state();
    panic!(
        "timed out waiting for: {what}\n  last_error={:?}\n  busy={} toast={:?}\n  painted={:?}",
        s.last_error,
        s.ui.busy,
        s.ui.toast.clone().map(|t| t.message),
        common::painted_text(harness),
    );
}

// --- shape inspection -------------------------------------------------------------

/// The solid color of a path stroke (icon strokes), if it paints anything.
fn solid_path_stroke(stroke: &egui::epaint::PathStroke) -> Option<Color32> {
    match stroke.color {
        ColorMode::Solid(c) if c != Color32::TRANSPARENT => Some(c),
        _ => None,
    }
}

/// The solid color of a plain rect stroke (window frames), if any.
fn solid_rect_stroke(stroke: &egui::Stroke) -> Option<Color32> {
    (stroke.color != Color32::TRANSPARENT).then_some(stroke.color)
}

/// Colors of stroked path primitives (what icon rendering emits) whose
/// geometry passes within `radius` of `near`.
fn stroked_colors_near(harness: &Harness<'_, AppState>, near: Pos2, radius: f32) -> Vec<Color32> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Path(p)
                if p.fill == Color32::TRANSPARENT
                    && p.points.iter().any(|pt| pt.distance(near) <= radius) =>
            {
                solid_path_stroke(&p.stroke)
            }
            _ => None,
        })
        .collect()
}

/// The window frame rect of a popup plus its top-level shape index: SURFACE
/// fill outlined by the LINE token. Window frames paint as
/// `[shadow, RectShape]` vecs; recurse to find them.
fn popup_chrome(harness: &Harness<'_, AppState>, what: &str) -> (Rect, usize) {
    fn find(shape: &Shape) -> Option<egui::Rect> {
        match shape {
            Shape::Rect(r)
                if r.fill == Palette::SURFACE
                    && solid_rect_stroke(&r.stroke) == Some(Palette::LINE) =>
            {
                Some(r.rect)
            }
            Shape::Vec(v) => v.iter().find_map(find),
            _ => None,
        }
    }
    for (i, clipped) in harness.output().shapes.iter().enumerate() {
        if let Some(rect) = find(&clipped.shape)
            && rect.width() > 300.0
            && rect.height() > 150.0
        {
            return (rect, i);
        }
    }
    panic!("{what} must render with popup chrome (SURFACE fill + LINE border)")
}

/// Every text galley painted after `after_idx` whose origin lies inside
/// `rect` — the popup's own content, excluding overlapping shell text below
/// it in paint order.
fn texts_inside_after(
    harness: &Harness<'_, AppState>,
    rect: Rect,
    after_idx: usize,
) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(i, clipped)| match &clipped.shape {
            Shape::Text(t) if i > after_idx && rect.contains(t.pos) => {
                Some(t.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

// --- Cycle 1: toasts paint their semantic kind ------------------------------------

const KIND_CASES: [(ToastKind, &str, Color32); 4] = [
    (
        ToastKind::Success,
        "op finished cleanly",
        Palette::STATE_SUCCESS,
    ),
    (
        ToastKind::Warning,
        "op needs attention",
        Palette::STATE_WARNING,
    ),
    (ToastKind::Error, "op failed badly", Palette::STATE_ERROR),
    (ToastKind::Info, "op has news", Palette::STATE_INFO),
];

#[test]
fn each_toast_kind_paints_its_semantic_color_and_icon() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);

    for (kind, msg, color) in KIND_CASES {
        {
            let st = harness.state_mut();
            st.ui.toast = Some(Toast {
                kind,
                message: msg.into(),
            });
            st.ui.toast_shown_at = None; // fresh auto-dismiss window per case
        }
        settle_quiet(&mut harness);

        assert_painted(&harness, msg);
        let origin =
            galley_origin(&harness, msg).unwrap_or_else(|| panic!("{kind:?} message not painted"));

        // Accent bar: the exact STATE_* token filled next to the message.
        let near_origin = Rect::from_center_size(origin, Vec2::new(140.0, 48.0));
        assert!(
            filled_rects(&harness)
                .iter()
                .any(|(r, c)| *c == color && r.intersects(near_origin)),
            "{kind:?} toast must paint its {color:?} accent bar"
        );

        // Matching icon: vector strokes tinted with the same token.
        assert!(
            stroked_colors_near(&harness, origin, 56.0).contains(&color),
            "{kind:?} toast must paint an icon stroked in {color:?}"
        );
    }
}

#[test]
fn dismiss_clears_the_toast() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    {
        let st = harness.state_mut();
        st.ui.toast = Some(Toast::success("dismissable op"));
        st.ui.toast_shown_at = None;
    }
    settle_quiet(&mut harness);
    assert_painted(&harness, "dismissable op");

    harness.get_by_label("Dismiss").click();
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.toast.is_none(),
        "Dismiss must clear the toast"
    );
    assert_not_painted(&harness, "dismissable op");
}

// --- Cycle 2: confirm cycle still executes / cancels -------------------------------

#[test]
fn confirm_ok_executes_the_destructive_action() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    harness.state_mut().ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
        name: "feature-a".into(),
    });
    settle_quiet(&mut harness);

    // Dialog chrome: header title + body copy + footer actions all painted.
    assert_painted(&harness, "Confirm");
    assert_painted(
        &harness,
        "Delete local branch 'feature-a'? This cannot be undone.",
    );
    assert_painted(&harness, "OK");
    assert_painted(&harness, "Cancel");

    harness.get_by_label("OK").click();
    pump_until(&mut harness, "feature-a deleted via OK", |s| {
        s.ui.confirm.is_none()
            && s.selected_root
                .as_ref()
                .and_then(|id| s.multi.by_id(id))
                .is_some_and(|r| r.branches.iter().all(|b| b.name != "feature-a"))
    });
}

#[test]
fn confirm_cancel_keeps_everything_intact() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    harness.state_mut().ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
        name: "feature-b".into(),
    });
    settle_quiet(&mut harness);

    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

    let s = harness.state();
    assert!(s.ui.confirm.is_none(), "Cancel must clear the prompt");
    let kept = s
        .selected_root
        .as_ref()
        .and_then(|id| s.multi.by_id(id))
        .is_some_and(|r| r.branches.iter().any(|b| b.name == "feature-b"));
    assert!(kept, "Cancel must leave the branch untouched");
}

// --- Cycle 3: VCS operations popup keeps its EXACT action set ----------------------

#[test]
fn vcs_popup_renders_popup_chrome_with_exact_action_set() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    harness.state_mut().ui.vcs_popup = true;
    settle_quiet(&mut harness);

    assert_painted(&harness, "VCS Operations");
    let (chrome, chrome_idx) = popup_chrome(&harness, "VCS Operations popup");

    // The action set inside the popup is exactly Action::all() — nothing
    // dropped, nothing added (the palette's extra shell actions live there).
    let mut inside = texts_inside_after(&harness, chrome, chrome_idx);
    inside.sort();
    inside.dedup();
    let mut expected: Vec<String> = vec!["VCS Operations".to_owned()];
    expected.extend(Action::all().iter().map(|a| a.label().to_owned()));
    expected.sort();
    assert_eq!(
        inside, expected,
        "VCS Operations popup must contain exactly its documented action set"
    );
}

// --- Cycle 4: command palette retains prior entries + gains three new ones ---------

#[test]
fn palette_retains_every_prior_entry_plus_three_new_ones() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    harness.state_mut().ui.command_palette = true;
    settle_quiet(&mut harness);

    assert_painted(&harness, "Find Action");
    // Every pre-existing entry survives (ADR-0011: nothing dropped)…
    for label in Action::all().iter().map(|a| a.label()) {
        assert_painted(&harness, label);
    }
    // …and the three new shell-navigation entries are listed too.
    for label in ["Go to Log", "Open Welcome", "Toggle Toolbar"] {
        assert_painted(&harness, label);
    }
}

/// Open the palette filtered to `query`, click `label`, and require that the
/// entry was really reachable through search + click.
fn run_palette_entry(harness: &mut Harness<'_, AppState>, query: &str, label: &str) {
    {
        let st = harness.state_mut();
        st.ui.command_palette = true;
        st.ui.command_query = query.to_owned();
    }
    settle_quiet(harness);
    assert_painted(harness, label);
    harness.get_by_label(label).click();
    settle_quiet(harness);
}

#[test]
fn go_to_log_is_reachable_from_the_palette() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    assert_eq!(harness.state().ui.tab, Tab::Commit);

    run_palette_entry(&mut harness, "log", "Go to Log");

    let s = harness.state();
    assert_eq!(s.ui.tab, Tab::Log, "Go to Log must switch to Git Log");
    assert!(!s.ui.command_palette, "palette must close after invoking");
}

#[test]
fn open_welcome_is_reachable_from_the_palette() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    assert!(!harness.state().ui.welcome_visible);

    run_palette_entry(&mut harness, "welcome", "Open Welcome");

    let s = harness.state();
    assert!(
        s.ui.welcome_visible,
        "Open Welcome must return to the Welcome page"
    );
    assert_painted(&harness, "A fast, keyboard-friendly Git client");
}

#[test]
fn toggle_toolbar_is_reachable_from_the_palette() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    assert!(harness.state().ui.show_toolbar);

    run_palette_entry(&mut harness, "toolbar", "Toggle Toolbar");

    let s = harness.state();
    assert!(!s.ui.show_toolbar, "Toggle Toolbar must hide the toolbar");
    assert_not_painted(&harness, "Run");
}
