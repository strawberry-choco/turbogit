use egui::epaint::ColorMode;
use egui::{Color32, Pos2, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::Queryable as _};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_text, settle_quiet,
    shell_harness_over,
};
use turbogit_app::state::{AppState, PendingConfirm, Tab, Toast, ToastKind};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::popups::Action;

// `repo_project` below writes no `user.*` into its repository, so the pinned
// identity is the only one the fixture's commits carry, and the confirm-dialog
// rows carry the author line: every call is pinned per call through the fixture
// crate's runner, because a headless machine has no global identity to borrow.

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git_with_env(repo, &["add", "."], &GitEnv::fixture());
    git_with_env(repo, &["commit", "-m", "init"], &GitEnv::fixture());
}

fn repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    let alpha = project.join("alpha");
    commit_readme(&alpha);
    git_with_env(&alpha, &["branch", "feature-a"], &GitEnv::fixture());
    git_with_env(&alpha, &["branch", "feature-b"], &GitEnv::fixture());
    (tmp, project)
}

fn feedback_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    shell_harness_over(AppState::new(project_dir), egui::vec2(1024.0, 768.0))
}

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
        painted_text(harness),
    );
}

fn solid_path_stroke(stroke: &egui::epaint::PathStroke) -> Option<Color32> {
    match stroke.color {
        ColorMode::Solid(c) if c != Color32::TRANSPARENT => Some(c),
        _ => None,
    }
}

fn solid_rect_stroke(stroke: &egui::Stroke) -> Option<Color32> {
    (stroke.color != Color32::TRANSPARENT).then_some(stroke.color)
}

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
            st.ui.toast_shown_at = None;
        }
        settle_quiet(&mut harness);

        assert_painted(&harness, msg);
        let origin =
            galley_origin(&harness, msg).unwrap_or_else(|| panic!("{kind:?} message not painted"));

        let near_origin = Rect::from_center_size(origin, Vec2::new(140.0, 48.0));
        assert!(
            filled_rects(&harness)
                .iter()
                .any(|(r, c)| *c == color && r.intersects(near_origin)),
            "{kind:?} toast must paint its {color:?} accent bar"
        );

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

#[test]
fn confirm_ok_executes_the_destructive_action() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    let root = harness.state().selected_root.clone().unwrap();
    harness.state_mut().ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
        root,
        name: "feature-a".into(),
    });
    settle_quiet(&mut harness);

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
    let root = harness.state().selected_root.clone().unwrap();
    harness.state_mut().ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
        root,
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

#[test]
fn vcs_popup_renders_popup_chrome_with_exact_action_set() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    harness.state_mut().ui.vcs_popup = true;
    settle_quiet(&mut harness);

    assert_painted(&harness, "VCS Operations");
    let (chrome, chrome_idx) = popup_chrome(&harness, "VCS Operations popup");

    let mut inside = texts_inside_after(&harness, chrome, chrome_idx);
    inside.sort();
    inside.dedup();
    let mut expected: Vec<String> = vec!["VCS Operations".to_owned()];
    let direct_offered = turbogit_ui::ui::push_control::direct_target(harness.state()).is_some();
    expected.extend(
        Action::all()
            .iter()
            .filter(|a| **a != Action::PushSimple || direct_offered)
            .map(|a| a.label().to_owned()),
    );
    expected.sort();
    assert_eq!(
        inside, expected,
        "VCS Operations popup must contain exactly its documented action set"
    );
}

#[test]
fn palette_retains_every_prior_entry_plus_two_new_ones() {
    let (_project, dir) = repo_project();
    let mut harness = feedback_harness(dir);
    harness.state_mut().ui.command_palette = true;
    settle_quiet(&mut harness);

    assert_painted(&harness, "Find Action");
    for label in Action::all().iter().map(|a| a.label()) {
        assert_painted(&harness, label);
    }
    for label in ["Go to Log", "Open Welcome"] {
        assert_painted(&harness, label);
    }
}

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
}
