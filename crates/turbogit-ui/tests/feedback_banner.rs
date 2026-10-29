use egui::{Color32, CornerRadius, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::Queryable as _};
use std::time::Duration;
use tempfile::TempDir;
use test_support::git_seed::repo_with_one_commit;
use test_support::harness::{
    assert_not_painted, assert_painted, settle_quiet, shell_harness_over, widget_harness,
};
use turbogit_app::banner::{Banner, BannerAction, BannerSeverity};
use turbogit_app::state::AppState;
use turbogit_ui::theme::{MARK_RADIUS, Palette};

const BAR_WIDTH: f32 = 3.0;
const BAR_HEIGHT: f32 = 18.0;

fn repo_project() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    repo_with_one_commit(&project, "alpha");
    (tmp, project)
}

fn feedback_harness(project_dir: std::path::PathBuf) -> Harness<'static, AppState> {
    shell_harness_over(AppState::new(project_dir), egui::vec2(1024.0, 768.0))
}

#[test]
fn banner_paints_message_and_every_action_label() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    let banner = Banner::new(BannerSeverity::Info, "Stash restored")
        .action(BannerAction::new("View diff", |_state| {}))
        .action(BannerAction::new("Dismiss", |_state| {}));
    harness.state_mut().ui.banner = Some(banner);
    settle_quiet(&mut harness);

    assert_painted(&harness, "Stash restored");
    assert_painted(&harness, "View diff");
    assert_painted(&harness, "Dismiss");
}

#[test]
fn banner_with_single_action_renders() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    let banner = Banner::new(BannerSeverity::Warning, "2 repos rejected the cascade push")
        .action(BannerAction::new("Review", |_state| {}));
    harness.state_mut().ui.banner = Some(banner);
    settle_quiet(&mut harness);

    assert_painted(&harness, "2 repos rejected the cascade push");
    assert_painted(&harness, "Review");
}

#[test]
fn no_banner_paints_no_banner_chrome() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    harness.state_mut().ui.banner = None;
    settle_quiet(&mut harness);

    assert_not_painted(&harness, "Stash restored");
    assert_not_painted(&harness, "View diff");
}

#[test]
fn clicking_a_banner_action_invokes_it_against_appstate() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    let banner = Banner::new(BannerSeverity::Info, "Stash restored").action(BannerAction::new(
        "Acknowledge",
        |state| {
            state.ui.toast = Some(turbogit_app::state::Toast::info("acknowledged"));
        },
    ));
    harness.state_mut().ui.banner = Some(banner);
    settle_quiet(&mut harness);
    assert_painted(&harness, "Acknowledge");

    harness.get_by_label("Acknowledge").click();

    let mut saw = false;
    for _ in 0..200 {
        harness.step();
        if let Some(t) = &harness.state().ui.toast
            && t.message == "acknowledged"
        {
            saw = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        saw,
        "clicking a banner action should run its closure against AppState; \
         current toast: {:?}",
        harness.state().ui.toast
    );
}

fn accent_bars_in<S>(
    harness: &Harness<'_, S>,
    color: Color32,
) -> Vec<(Rect, CornerRadius, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape)
                if rect_shape.fill == color
                    && rect_shape.rect.size() == Vec2::new(BAR_WIDTH, BAR_HEIGHT) =>
            {
                Some((rect_shape.rect, rect_shape.corner_radius, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

fn accent_bar_harness(color: Color32) -> Harness<'static, ()> {
    widget_harness(egui::vec2(240.0, 60.0), move |ui| {
        ui.horizontal(|ui| {
            turbogit_ui::ui::widgets::accent_bar(ui, color);
            ui.label("after");
        });
    })
}

#[test]
fn accent_bar_primitive_paints_one_mark_rounded_strip_in_the_given_color() {
    let color = Palette::STATE_WARNING;
    let mut harness = accent_bar_harness(color);
    harness.step();

    let bars = accent_bars_in(&harness, color);
    assert_eq!(
        bars.len(),
        1,
        "the accent bar paints exactly one filled rect; painted: {bars:?}"
    );
    let (rect, radius, fill) = bars[0];
    assert_eq!(
        fill, color,
        "the strip paints the colour it was given, verbatim"
    );
    assert_eq!(
        rect.size(),
        Vec2::new(BAR_WIDTH, BAR_HEIGHT),
        "the strip keeps its spec size on every host"
    );
    assert_eq!(
        radius,
        CornerRadius::same(MARK_RADIUS),
        "a strip this small rounds at MARK_RADIUS, not CONTROL_RADIUS"
    );
}

#[test]
fn banner_paints_its_accent_bar_in_the_severity_color() {
    const CASES: [(BannerSeverity, Color32); 4] = [
        (BannerSeverity::Info, Palette::STATE_INFO),
        (BannerSeverity::Warning, Palette::STATE_WARNING),
        (BannerSeverity::Error, Palette::STATE_ERROR),
        (BannerSeverity::Success, Palette::STATE_SUCCESS),
    ];

    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    for (severity, color) in CASES {
        harness.state_mut().ui.banner = Some(Banner::new(severity, "Stash restored"));
        settle_quiet(&mut harness);

        let bars = accent_bars_in(&harness, color);
        assert_eq!(
            bars.len(),
            1,
            "{severity:?} banner must paint exactly one accent bar in {color:?}; \
             painted: {bars:?}"
        );
        assert_eq!(
            bars[0].1,
            CornerRadius::same(MARK_RADIUS),
            "{severity:?} banner's strip rounds at MARK_RADIUS"
        );
    }
}
