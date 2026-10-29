use std::path::PathBuf;
use std::sync::OnceLock;

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{filled_rects, painted_text, settle, shell_harness_over};
use turbogit_app::state::AppState;
use turbogit_ui::theme::Palette;

const ROW_H: f32 = 24.0;

const MAIN_LEN: usize = 150;

fn graph_region(harness: &Harness<'_, AppState>) -> Rect {
    let branches = filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 200.0 && r.width() <= 220.0)
        .map(|(r, _)| r)
        .expect("branches pane card not painted");
    let body_right = filled_rects(harness)
        .iter()
        .map(|(r, _)| r.right())
        .fold(f32::NEG_INFINITY, f32::max);
    Rect::from_min_max(
        Pos2::new(branches.right(), branches.top()),
        Pos2::new(body_right - 330.0, branches.bottom()),
    )
}

fn text_center_y(harness: &Harness<'_, AppState>, text: &str) -> Option<f32> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => {
                Some(shape.pos.y + shape.galley.size().y / 2.0)
            }
            _ => None,
        })
}

fn row_top_of(harness: &Harness<'_, AppState>, text: &str) -> Option<f32> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => {
                Some(shape.pos.y - (ROW_H - shape.galley.size().y) / 2.0)
            }
            _ => None,
        })
}

fn circles(harness: &Harness<'_, AppState>) -> Vec<(Pos2, f32, Color32, Color32, Rect)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Circle(circle) => Some((
                circle.center,
                circle.radius,
                circle.fill,
                circle.stroke.color,
                clipped.clip_rect,
            )),
            _ => None,
        })
        .collect()
}

const ROW_NODE_R: f32 = 4.0;

fn built_rows(harness: &Harness<'_, AppState>) -> Vec<(f32, Color32)> {
    let graph = graph_region(harness);
    let mut rows: Vec<(f32, Color32)> = circles(harness)
        .into_iter()
        .filter(|(centre, radius, _, _, _)| {
            *radius == ROW_NODE_R && graph.x_range().contains(centre.x)
        })
        .map(|(centre, _, fill, stroke, _)| {
            let colour = if fill != Color32::TRANSPARENT {
                fill
            } else {
                stroke
            };
            (centre.y, colour)
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.dedup_by(|a, b| a.0 == b.0);
    rows
}

fn list_viewport(harness: &Harness<'_, AppState>) -> Rect {
    circles(harness)
        .into_iter()
        .find(|(centre, radius, _, _, _)| {
            *radius == ROW_NODE_R && graph_region(harness).x_range().contains(centre.x)
        })
        .map(|(_, _, _, _, clip)| clip)
        .expect("the list's rows are painted under a clip rect")
}

fn lane_node_colour(harness: &Harness<'_, AppState>, text: &str) -> Option<Color32> {
    let y = text_center_y(harness, text)?;
    built_rows(harness)
        .into_iter()
        .find(|(node_y, _)| (node_y - y).abs() < 5.0)
        .map(|(_, colour)| colour)
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == needle),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

fn main_time(i: usize) -> i64 {
    1000 + 2 * i as i64
}

struct Long {
    _tmp: tempfile::TempDir,
    project: PathBuf,
}

fn long_history() -> &'static Long {
    static LONG: OnceLock<Long> = OnceLock::new();
    LONG.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path().join("project");
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).expect("alpha dir");
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(
            &repo,
            &["config", "user.email", "t@example.com"],
            &GitEnv::fixture(),
        );
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());

        let data = |stream: &mut String, s: &str| {
            stream.push_str(&format!("data {}\n{s}", s.len()));
        };
        let mut stream = String::new();
        stream.push_str("commit refs/heads/main\nmark :1\n");
        stream.push_str("author t <t@example.com> 1000 +0000\n");
        stream.push_str("committer t <t@example.com> 1000 +0000\n");
        data(&mut stream, "c0\n");
        stream.push_str("M 644 inline f.txt\n");
        data(&mut stream, "body 0\n");
        for i in 1..MAIN_LEN {
            stream.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
            stream.push_str(&format!(
                "author t <t@example.com> {} +0000\n",
                main_time(i)
            ));
            stream.push_str(&format!(
                "committer t <t@example.com> {} +0000\n",
                main_time(i)
            ));
            data(&mut stream, &format!("c{i}\n"));
            stream.push_str(&format!("from :{i}\n"));
            stream.push_str("M 644 inline f.txt\n");
            data(&mut stream, &format!("body {i}\n"));
        }
        let tip_mark = MAIN_LEN + 2;
        for (offset, subject) in [(1usize, "topic root\n"), (2, "topic tip\n")] {
            let mark = MAIN_LEN + offset;
            stream.push_str(&format!("commit refs/heads/topic\nmark :{mark}\n"));
            let when = if offset == 1 {
                main_time(MAIN_LEN - 10) - 1
            } else {
                main_time(MAIN_LEN - 10) + 1
            };
            stream.push_str(&format!("author t <t@example.com> {when} +0000\n"));
            stream.push_str(&format!("committer t <t@example.com> {when} +0000\n"));
            data(&mut stream, subject);
            if offset == 1 {
                stream.push_str(&format!("from :{}\n", MAIN_LEN - 10 + 1));
            } else {
                stream.push_str(&format!("from :{}\n", MAIN_LEN + 1));
            }
            stream.push_str("M 644 inline t.txt\n");
            data(&mut stream, &format!("topic body {offset}\n"));
        }
        let merge_mark = MAIN_LEN + 3;
        stream.push_str(&format!("commit refs/heads/main\nmark :{merge_mark}\n"));
        stream.push_str("author t <t@example.com> 3000 +0000\n");
        stream.push_str("committer t <t@example.com> 3000 +0000\n");
        data(&mut stream, "merge topic into main\n");
        stream.push_str(&format!("from :{MAIN_LEN}\n"));
        stream.push_str(&format!("merge :{tip_mark}\n"));
        stream.push_str("M 644 inline m.txt\n");
        data(&mut stream, "merge body\n");

        let mut child = std::process::Command::new("git")
            .arg("fast-import")
            .current_dir(&repo)
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn git fast-import");
        use std::io::Write as _;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stream.as_bytes())
            .expect("write fast-import stream");
        let out = child.wait_with_output().expect("wait fast-import");
        assert!(
            out.status.success(),
            "git fast-import failed with {:?}; stderr:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        git_with_env(
            &repo,
            &["reset", "-q", "--hard", "main"],
            &GitEnv::fixture(),
        );
        Long { _tmp: tmp, project }
    })
}

fn long_log_harness() -> Harness<'static, AppState> {
    let long = long_history();
    let mut state = AppState::new(long.project.clone());
    assert_eq!(state.multi.roots.len(), 1, "one root discovered");
    let root = state.multi.roots[0].id.clone();
    state.fetch_log(root.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    settle_until(&mut harness, "c149");
    for _ in 0..6 {
        if !harness.query_by_label("Load more").is_some() {
            break;
        }
        harness.get_by_label("Load more").click();
        settle(&mut harness);
    }
    settle(&mut harness);
    harness
}

#[track_caller]
fn settle_until(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..200 {
        harness.step();
        if painted_text(harness).iter().any(|t| t == needle) {
            settle(harness);
            return;
        }
    }
    panic!("`{needle}` was never painted");
}

/// Scroll the commit list by `delta` pixels with one wheel gesture over the
/// graph. The delta is wrapped in a touch phase because that is the one path
/// egui applies whole in a single frame, a bare wheel delta is spread over
/// several frames, and a list that pages while it is still scrolling reads as
/// two arrivals at the bottom rather than one.
fn wheel(harness: &mut Harness<'static, AppState>, delta: f32) {
    let toolbar = harness.get_by_label("Search commits").rect();
    harness.hover_at(egui::pos2(toolbar.center().x, toolbar.bottom() + 60.0));
    harness.step();
    for phase in [
        egui::TouchPhase::Start,
        egui::TouchPhase::Move,
        egui::TouchPhase::End,
    ] {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, delta),
            phase,
            modifiers: egui::Modifiers::default(),
        });
        harness.step();
    }
    settle(harness);
}

const LANE_0_BLUE: Color32 = Color32::from_rgb(80, 140, 230);
const LANE_1_PINK: Color32 = Color32::from_rgb(220, 120, 140);

const TOPIC_TIP_ROW: usize = 10;
const TOPIC_ROOT_ROW: usize = 12;

fn shown_rows(harness: &Harness<'_, AppState>) -> usize {
    painted_text(harness)
        .into_iter()
        .find_map(|t| t.strip_suffix(" shown")?.parse().ok())
        .expect("the pane's row count line is painted")
}

#[test]
fn a_long_window_builds_only_the_rows_its_viewport_can_hold() {
    let mut harness = long_log_harness();
    let window = shown_rows(&harness);

    for (label, wheel_delta) in [("at the top of the list", 0.0), ("ten rows down", -300.0)] {
        if wheel_delta != 0.0 {
            wheel(&mut harness, wheel_delta);
        }
        let viewport = list_viewport(&harness);
        let pitch = {
            let rows = built_rows(&harness);
            assert!(
                rows.len() >= 2,
                "{label}: a frame with fewer than two rows cannot have a pitch"
            );
            rows[1].0 - rows[0].0
        };
        let bound = (viewport.height() / pitch).ceil() as usize + 2;
        let built = built_rows(&harness).len();

        assert!(
            window >= 4 * bound,
            "{label}: the fixture's {window}-row window is not deep enough for the \
             {bound}-row bound to say anything"
        );
        assert!(
            built <= bound,
            "{label}: {built} rows built for a {:.0}px viewport at a {pitch:.0}px pitch \
             (bound {bound}), out of a {window}-row window",
            viewport.height()
        );
        assert!(
            built + 2 >= (viewport.height() / pitch).floor() as usize,
            "{label}: only {built} rows for a {:.0}px viewport at a {pitch:.0}px pitch — \
             the bound is not being satisfied by an empty list",
            viewport.height()
        );
    }

    assert_eq!(
        row_top_of(&harness, "topic tip"),
        Some(list_viewport(&harness).top()),
        "ten rows down, the side branch's tip must sit at the viewport's top edge"
    );
    assert_eq!(
        TOPIC_TIP_ROW, 10,
        "the fixture's row 10 is the side branch's tip"
    );
}

#[test]
fn a_commit_scrolled_outside_the_viewport_paints_nothing() {
    let mut harness = long_log_harness();
    let head_subject = "merge topic into main";
    let oldest_subject = "c0";
    let window = shown_rows(&harness);
    assert!(
        window > 100,
        "the fixture's window must be far deeper than its viewport, got {window}"
    );

    assert_painted(&harness, head_subject);
    assert!(
        lane_node_colour(&harness, head_subject).is_some(),
        "the head commit's lane node must paint before the scroll"
    );
    assert!(
        text_center_y(&harness, oldest_subject).is_none(),
        "the window's last row is far below the viewport at the top of the list"
    );

    wheel(&mut harness, -300.0);
    assert!(
        text_center_y(&harness, head_subject).is_none(),
        "a commit nine rows above the viewport must not paint"
    );
    assert!(
        lane_node_colour(&harness, head_subject).is_none(),
        "a commit above the viewport must not paint a lane node either"
    );
    assert_eq!(
        built_rows(&harness)
            .first()
            .map(|(y, _)| *y)
            .map(|y| y - ROW_H / 2.0),
        Some(list_viewport(&harness).top()),
        "the topmost row built is the one at the viewport's top edge, not the head of the list"
    );
    assert!(
        text_center_y(&harness, oldest_subject).is_none(),
        "the window's last row is far below the viewport and must not paint"
    );

    wheel(&mut harness, -100_000.0);
    assert_painted(&harness, oldest_subject);
    assert!(
        text_center_y(&harness, head_subject).is_none(),
        "the head of the list is still far above the viewport at the end of the window"
    );
    let viewport = list_viewport(&harness);
    let rows = built_rows(&harness);
    let pitch = rows.windows(2).map(|w| w[1].0 - w[0].0).fold(0.0, f32::max);
    let above = rows
        .iter()
        .filter(|(y, _)| y + ROW_H / 2.0 <= viewport.top())
        .count();
    let below = rows
        .iter()
        .filter(|(y, _)| y - ROW_H / 2.0 >= viewport.bottom())
        .count();
    assert_eq!(
        (above, below),
        (1, 0),
        "at most one row may be built past the viewport's top edge and none past \
         its bottom; got {above} above and {below} below {viewport:?} \
         (row centres {:?})",
        rows.iter().map(|(y, _)| *y).collect::<Vec<_>>()
    );
    let fits = (viewport.height() / pitch).ceil() as usize;
    assert!(
        rows.len() <= fits + 1,
        "the list built {} rows for a {:.0}px viewport at a {pitch:.0}px pitch \
         — more than one viewport's worth",
        rows.len(),
        viewport.height()
    );
}

#[test]
fn a_side_branch_commit_keeps_its_lane_colour_across_scrolling() {
    let mut harness = long_log_harness();
    let mainline = "c149";
    let tip = "topic tip";
    let side = "topic root";

    let side_at_top = lane_node_colour(&harness, side);
    assert_eq!(
        side_at_top,
        Some(LANE_1_PINK),
        "the side branch's root must take the lane the merge opened, not lane 0"
    );
    assert_eq!(
        lane_node_colour(&harness, mainline),
        Some(LANE_0_BLUE),
        "a mainline commit keeps lane 0"
    );
    assert_eq!(
        lane_node_colour(&harness, tip),
        Some(LANE_1_PINK),
        "the merge's second parent is the side branch too"
    );

    let pitch = {
        let rows = built_rows(&harness);
        rows[1].0 - rows[0].0
    };
    wheel(&mut harness, -(TOPIC_ROOT_ROW as f32 * pitch));
    assert_eq!(
        row_top_of(&harness, side),
        Some(list_viewport(&harness).top()),
        "the side branch's root must now be the first row the viewport shows"
    );
    assert!(
        text_center_y(&harness, "merge topic into main").is_none(),
        "the merge is scrolled out, so a walk scoped to the viewport could not see it"
    );
    assert_eq!(
        lane_node_colour(&harness, side),
        Some(LANE_1_PINK),
        "a commit's lane colour must not follow it up and down the list"
    );
    assert_ne!(
        lane_node_colour(&harness, side),
        lane_node_colour(&harness, mainline),
        "the two lanes are different colours, so the assertion is not vacuous"
    );

    wheel(&mut harness, -100_000.0);
    assert!(
        text_center_y(&harness, side).is_none(),
        "scrolled to the end of the window, the side branch's root is far above"
    );
    wheel(&mut harness, 100_000.0);
    assert_eq!(
        row_top_of(&harness, "merge topic into main"),
        Some(list_viewport(&harness).top()),
        "the list is back at the top"
    );
    assert_eq!(
        lane_node_colour(&harness, side),
        side_at_top,
        "the same commit paints the same lane colour it did before the list scrolled away"
    );
}
