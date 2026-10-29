use std::path::PathBuf;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;
use test_support::git_seed::repo_with_origin;
use test_support::harness::{
    assert_not_painted, assert_painted, painted_galleys, settle_quiet, shell_harness_over,
};
use turbogit_app::activity::{ActivityEntry, ActivityKind, TimeWindow};
use turbogit_app::state::AppState;

fn repo_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_origin(&project, "alpha");
    (tmp, project, alpha)
}

fn activity_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    shell_harness_over(AppState::new(project_dir), egui::vec2(1024.0, 768.0))
}

fn entry(minutes_ago: i64, repo: Option<&str>, message: &str, kind: ActivityKind) -> ActivityEntry {
    ActivityEntry {
        at: chrono::Local::now() - chrono::Duration::minutes(minutes_ago),
        repo: repo.map(str::to_string),
        message: message.to_string(),
        kind,
    }
}

fn plant(harness: &mut Harness<'_, turbogit_app::state::AppState>, entries: Vec<ActivityEntry>) {
    {
        let st = harness.state_mut();
        st.ui.activity.expanded = true;
        for e in entries {
            st.ui.activity.push(e);
        }
    }
    settle_quiet(harness);
}

#[test]
fn panel_paints_entries_with_timestamp_repo_and_outcome() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![
            entry(0, Some("alpha"), "Fetch from origin", ActivityKind::Success),
            entry(
                1,
                Some("alpha"),
                "Push origin/main: rejected",
                ActivityKind::Error,
            ),
        ],
    );

    assert_painted(&harness, "LOG");
    assert_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "Push origin/main: rejected");
    assert_painted(&harness, "alpha");
    assert_painted(&harness, "All repos");
    assert_painted(&harness, "All time");
    assert_painted(&harness, "Clear feed");

    assert_eq!(
        turbogit_ui::ui::activity_panel::kind_color(ActivityKind::Success),
        turbogit_ui::theme::Palette::STATE_SUCCESS
    );
    assert_eq!(
        turbogit_ui::ui::activity_panel::kind_color(ActivityKind::Warning),
        turbogit_ui::theme::Palette::STATE_WARNING
    );
    assert_eq!(
        turbogit_ui::ui::activity_panel::kind_color(ActivityKind::Error),
        turbogit_ui::theme::Palette::STATE_ERROR
    );
}

#[test]
fn empty_feed_paints_a_hint() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);
    harness.state_mut().ui.activity.expanded = true;
    settle_quiet(&mut harness);
    assert_painted(&harness, "LOG");
    assert_painted(&harness, "No activity yet");
}

#[test]
fn collapse_and_expand_retain_entries() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![entry(
            0,
            Some("alpha"),
            "Fetch from origin",
            ActivityKind::Success,
        )],
    );
    assert!(harness.state().ui.activity.expanded);
    assert_painted(&harness, "Fetch from origin");

    harness.get_by_label("Collapse log").click();
    settle_quiet(&mut harness);
    assert!(!harness.state().ui.activity.expanded);
    assert_not_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "LOG");

    assert_eq!(harness.state().ui.activity.entries.len(), 1);

    harness.get_by_label("Expand log").click();
    settle_quiet(&mut harness);
    assert!(harness.state().ui.activity.expanded);
    assert_painted(&harness, "Fetch from origin");
}

#[test]
fn clear_empties_the_feed() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![entry(
            0,
            Some("alpha"),
            "Fetch from origin",
            ActivityKind::Success,
        )],
    );
    assert_painted(&harness, "Fetch from origin");

    harness.get_by_label("Clear feed").click();
    settle_quiet(&mut harness);

    assert!(harness.state().ui.activity.entries.is_empty());
    assert_not_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "No activity yet");
}

#[test]
fn repo_filter_narrows_the_painted_feed() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![
            entry(0, Some("alpha"), "Fetch from origin", ActivityKind::Success),
            entry(0, Some("beta"), "Pull upstream", ActivityKind::Success),
        ],
    );
    assert_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "Pull upstream");

    harness.get_by_label("All repos").click();
    settle_quiet(&mut harness);

    assert_eq!(
        harness.state().ui.activity.repo_filter.as_deref(),
        Some("alpha")
    );
    assert_painted(&harness, "Fetch from origin");
    assert_not_painted(&harness, "Pull upstream");
    assert_eq!(harness.state().ui.activity.entries.len(), 2);
}

#[test]
fn time_window_filter_hides_old_entries() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![
            entry(0, Some("alpha"), "fresh fetch", ActivityKind::Success),
            entry(40, Some("alpha"), "old fetch", ActivityKind::Success),
        ],
    );
    assert_painted(&harness, "fresh fetch");
    assert_painted(&harness, "old fetch");

    harness.get_by_label("All time").click();
    settle_quiet(&mut harness);

    assert_eq!(harness.state().ui.activity.window, TimeWindow::Last30Min);
    assert_painted(&harness, "fresh fetch");
    assert_not_painted(&harness, "old fetch");
}

#[test]
fn toggle_hit_target_contains_the_painted_chevron() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);
    settle_quiet(&mut harness);

    let hit = harness.get_by_label("Expand log").rect();

    let ink2 = turbogit_ui::theme::Palette::INK_2;
    let inside: Vec<_> = test_support::harness::painted_paths(&harness)
        .into_iter()
        .filter(|(rect, color)| *color == ink2 && hit.contains(rect.center()))
        .collect();
    assert_eq!(
        inside.len(),
        1,
        "exactly one chevron glyph must be painted inside the toggle's hit \
         rect {hit:?}; INK_2 paths inside: {inside:?}"
    );

    let glyph = inside[0].0.center();
    harness.event(egui::Event::PointerMoved(glyph));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: glyph,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.activity.expanded,
        "clicking the painted chevron must expand the panel"
    );
}

/// The corner radius of the one painted `Shape::Rect` at `rect` that `pick`
/// selects, with a pixel of geometry tolerance (the kittest rect round-trips
/// through screen pixels before it comes back to logical coordinates).
#[track_caller]
fn corner_radius_at<S>(
    harness: &Harness<'_, S>,
    rect: egui::Rect,
    what: &str,
    pick: impl Fn(&egui::epaint::RectShape) -> bool,
) -> egui::CornerRadius {
    let radii: Vec<_> = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rs)
                if pick(rs)
                    && (rs.rect.min - rect.min).abs().max_elem() < 1.0
                    && (rs.rect.max - rect.max).abs().max_elem() < 1.0 =>
            {
                Some(rs.corner_radius)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        radii.len(),
        1,
        "expected exactly one {what} painted at {rect:?}; {} matched (radii: {radii:?})",
        radii.len()
    );
    radii[0]
}

#[test]
fn title_control_hover_fill_and_focus_ring_share_one_radius() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);
    settle_quiet(&mut harness);

    harness.get_by_label("Expand log").focus();
    settle_quiet(&mut harness);
    let hit = harness.get_by_label("Expand log").rect();
    harness.event(egui::Event::PointerMoved(hit.center()));
    settle_quiet(&mut harness);

    let fill = corner_radius_at(&harness, hit, "title control hover fill", |rs| {
        rs.fill == turbogit_ui::theme::Palette::SURFACE_2
    });
    let ring = corner_radius_at(
        &harness,
        hit.expand(1.0),
        "title control focus ring",
        |rs| rs.stroke.width >= 1.0 && rs.stroke.color == turbogit_ui::theme::Palette::BRAND,
    );
    let expected = egui::CornerRadius::same(turbogit_ui::theme::CONTROL_RADIUS);

    assert_eq!(
        fill, expected,
        "the title control's hover fill must round at CONTROL_RADIUS, the \
         radius its focus ring strokes at"
    );
    assert_eq!(
        ring, expected,
        "the title control's focus ring must round at CONTROL_RADIUS"
    );
    assert_eq!(
        fill, ring,
        "the title control's hover fill ({fill:?}) and its focus ring \
         ({ring:?}) must share one corner radius"
    );
}

#[test]
fn real_fetch_lands_in_the_panel() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);
    settle_quiet(&mut harness);

    harness.state_mut().ui.activity.expanded = true;
    settle_quiet(&mut harness);
    harness.state_mut().ui.command_palette = true;
    harness.state_mut().ui.command_query = "fetch".to_string();
    settle_quiet(&mut harness);
    harness.get_by_label("Fetch").click();
    settle_quiet(&mut harness);

    let mut landed = false;
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        let stored = harness
            .state()
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.message.starts_with("Fetch") && e.kind == ActivityKind::Success);
        if stored {
            landed = true;
            break;
        }
    }
    assert!(
        landed,
        "real fetch should be recorded as an activity entry; entries: {:?}",
        harness.state().ui.activity.entries
    );
    let galleys = painted_galleys(&harness);
    let outcome = galleys
        .iter()
        .find(|g| g.text.starts_with("Fetch ·"))
        .unwrap_or_else(|| panic!("the fetch outcome must be painted; painted:\n{galleys:#?}"));
    let same_row = |g: &test_support::harness::PaintedGalley| {
        g.text == "alpha" && g.rect.max.y > outcome.rect.min.y && g.rect.min.y < outcome.rect.max.y
    };
    let label = galleys
        .iter()
        .find(|g| same_row(g))
        .unwrap_or_else(|| panic!("the feed row's repo label must paint; painted:\n{galleys:#?}"));
    assert!(
        label.rect.left() < outcome.rect.left(),
        "the repo label leads its outcome on the feed row: label {label:?}, outcome {outcome:?}"
    );
}
