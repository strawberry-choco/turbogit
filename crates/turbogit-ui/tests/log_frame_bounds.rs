//! Log-view-scaling 07 — a frame's work is bounded by the viewport, and a
//! commit's lane colour survives scrolling.
//!
//! These are assertions about PAINTED OUTPUT and nothing else: no row model is
//! read, no counter is added to production code, and every claim is made at the
//! same seam the log rendering suite already uses — the real render function,
//! end to end, over a real repository, read out of `harness.output()`.
//!
//! They live in their own file rather than in `git_log.rs` because they need a
//! different fixture: a 153-commit history with a merge whose second parent sits
//! mid-list, which is the only shape where a viewport-scoped lane walk would
//! paint a different colour for the same commit. `git_log.rs`'s three-commit
//! decorated history is kept untouched, and its 28 tests are the unchanged
//! evidence that a row's appearance survived virtualization.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::harness::{filled_rects, painted_text, settle, shell_harness_over};
use turbogit_app::state::AppState;
use turbogit_ui::theme::Palette;

/// The row pitch's fixed part — a commit row allocates exactly this height
/// (`crate::theme::FILE_ROW_HEIGHT`); egui adds `item_spacing.y` on top, which
/// the tests below MEASURE from painted output rather than assume.
const ROW_H: f32 = 24.0;

/// How many linear commits sit on `main` before the topic branch leaves it.
const MAIN_LEN: usize = 150;

// --- painted-output probes ----------------------------------------------------
// The painted queries are the shared `test_support::harness` ones. What is left
// here is what no shared query answers: the graph band's geometry, the row lattice
// read off the painted circles, and the exact-match label probe below.

/// The central graph band between the branches pane and the right column — the
/// region a commit row lives in, and the only one the row count is read from
/// (the branches pane paints its own dots).
///
/// The branches pane's band is found by the **card's** surface token, not the
/// raised one it used to be: the log's three carded panes wear
/// `CONTENT_BG` at the card radius with no stroke (design-system v2, R2), and
/// the branches pane is the narrow one at the far left. Nothing about the
/// geometry this derives changed — the band is still ~210px wide at the same
/// left edge — so this is the surface token following the pane, stated here
/// because "the graph region" silently became unmeasurable if it was not.
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

/// A painted text's VERTICAL CENTRE. Every cell of a commit row is centred on
/// its row's middle, so a row's text centre is that row's y — which is how a
/// test can name a row and find the shapes painted on it without asking the row
/// model where it is.
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

/// The top edge of the row whose text is painted at `text`: a row allocates
/// [`ROW_H`] and centres its cells in it, so the cell's top sits
/// `(ROW_H - cell_height) / 2` below the row's top.
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

/// Every circle painted by the last frame, as `(centre, radius, fill, stroke,
/// clip_rect)` — the whole population, so a test can see what the frame
/// contains and not only what it expected.
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

/// The node radius a commit row paints. It is the frame's own signature: the
/// only other circle the shell draws is the branches pane's 3.5px root stripe
/// dot, at a different x and under its own clip. Every commit row — linear or
/// merge — paints exactly one circle of this radius in the graph's node column,
/// so counting them counts the ROWS THE LIST BUILT, whether or not the scroll
/// area's clip culls the one it paints past the bottom edge.
const ROW_NODE_R: f32 = 4.0;

/// The rows the list built this frame, as `(row centre y, lane colour)`, in
/// display order — the graph's own circles, ignoring the clip rect.
///
/// Ignoring the clip is the point. A row the scroll area builds but does not
/// show is still in the frame's output, and a list that laid out its whole
/// 153-row window would paint 153 of them. Culling happens in the tessellator,
/// so a clip-filtered count would report the same number either way and could
/// not tell a paged list from an unpaged one.
fn built_rows(harness: &Harness<'_, AppState>) -> Vec<(f32, Color32)> {
    let graph = graph_region(harness);
    let mut rows: Vec<(f32, Color32)> = circles(harness)
        .into_iter()
        .filter(|(centre, radius, _, _, _)| {
            *radius == ROW_NODE_R && graph.x_range().contains(centre.x)
        })
        // A linear commit's node is filled with its lane colour; a merge's is a
        // ring in it, so the stroke carries the colour instead.
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

/// The scroll area's viewport, read off the clip rect the row nodes were painted
/// under. This is the list's own answer to "how tall am I", so no geometry is
/// assumed: the bound below is derived from the viewport and the measured pitch.
fn list_viewport(harness: &Harness<'_, AppState>) -> Rect {
    circles(harness)
        .into_iter()
        .find(|(centre, radius, _, _, _)| {
            *radius == ROW_NODE_R && graph_region(harness).x_range().contains(centre.x)
        })
        .map(|(_, _, _, _, clip)| clip)
        .expect("the list's rows are painted under a clip rect")
}

/// The lane node colour painted on the row whose text is `text`.
fn lane_node_colour(harness: &Harness<'_, AppState>, text: &str) -> Option<Color32> {
    let y = text_center_y(harness, text)?;
    built_rows(harness)
        .into_iter()
        // Well inside a row's own band: a row is 24px tall, so ±5 cannot reach
        // the neighbouring row's node.
        .find(|(node_y, _)| (node_y - y).abs() < 5.0)
        .map(|(_, colour)| colour)
}

/// Assert some painted text galley is EXACTLY `needle`. Not the shared
/// `assert_painted`: the call sites pass whole commit SUBJECTS, which a
/// containment match could satisfy with a longer galley that merely mentions them.
#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == needle),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

// --- Fixture: a long history with a merge whose side branch sits mid-list ------

/// `main`'s commit `i`'s timestamp: two seconds per commit, so every gap has a
/// free second in it for the side branch to occupy.
fn main_time(i: usize) -> i64 {
    1000 + 2 * i as i64
}

/// A `git` runner that pins the commit identity on every invocation.
///
/// Not `test_support::git_seed::git`, which takes no per-call env: the row order here
/// is decided by explicit `1000 + 2n` epoch seconds in the `fast-import` stream, and
/// pinning `GIT_AUTHOR_*`/`GIT_COMMITTER_*` fixes the repository's own identity too
/// rather than borrowing the machine's global config.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The seeded repository, kept alive for the whole test binary.
///
/// Not a `git_seed` recipe: 153 commits with a mid-list merge and hand-written
/// timestamps has no equivalent upstream.
struct Long {
    _tmp: tempfile::TempDir,
    project: PathBuf,
}

/// One repository, seeded once for the whole binary in a single `fast-import`
/// batch: a linear `main` of [`MAIN_LEN`] commits, a `topic` branch leaving it
/// near its root, and a merge of `topic` back into `main` at the tip.
///
/// The shape is chosen for the lane walk. Newest-first the window reads
/// `[merge, c149 … c141, topic_tip, topic_root, c140 … c0]`, so:
///
/// - the merge is row 0, and it OPENS a second lane for its second parent;
/// - `topic_root` sits at row 12, far enough from the merge that a walk scoped
///   to the painted rows would re-derive its colour from whatever the viewport
///   happens to start at — which is what makes it the right commit for the
///   lane-stability assertion;
/// - 150+ commits means the window is three times the viewport's height, so
///   "the frame's work" has room to differ from "the window's size".
fn long_history() -> &'static Long {
    static LONG: OnceLock<Long> = OnceLock::new();
    LONG.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path().join("project");
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).expect("alpha dir");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "t"]);

        // One `data <len>` block per payload, so a multi-digit subject or body
        // does not desync the stream.
        let data = |stream: &mut String, s: &str| {
            stream.push_str(&format!("data {}\n{s}", s.len()));
        };
        // `c0` gets a fractional time: fast-import takes integers, so the
        // branch point is `1000` and everything after it is `1001 + n`.
        let mut stream = String::new();
        stream.push_str("commit refs/heads/main\nmark :1\n");
        stream.push_str("author t <t@example.com> 1000 +0000\n");
        stream.push_str("committer t <t@example.com> 1000 +0000\n");
        data(&mut stream, "c0\n");
        stream.push_str("M 644 inline f.txt\n");
        data(&mut stream, "body 0\n");
        for i in 1..MAIN_LEN {
            stream.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
            // `main`'s commits are spaced TWO seconds apart, so there is an odd
            // second in every gap for the side branch to occupy — a tie would be
            // broken by commit id, which is not a position anybody can reason
            // about.
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
        // The topic branch leaves `main` at its root (`:1`) and adds two
        // commits timed after `c140`, so it lands between `c141` and `c140` in
        // the newest-first window — a lane that has to be walked through the
        // merge, not inferred from a position.
        let tip_mark = MAIN_LEN + 2;
        for (offset, subject) in [(1usize, "topic root\n"), (2, "topic tip\n")] {
            let mark = MAIN_LEN + offset;
            stream.push_str(&format!("commit refs/heads/topic\nmark :{mark}\n"));
            // The branch leaves `main` at `c140`; its root takes the free second
            // just below that commit and its tip the one just above it, so the
            // newest-first window reads
            // `[merge, c149 … c141, topic tip, c140, topic root, c139 … c0]`.
            let when = if offset == 1 {
                main_time(MAIN_LEN - 10) - 1
            } else {
                main_time(MAIN_LEN - 10) + 1
            };
            stream.push_str(&format!("author t <t@example.com> {when} +0000\n"));
            stream.push_str(&format!("committer t <t@example.com> {when} +0000\n"));
            data(&mut stream, subject);
            // `from` is the first parent; a second parent is spelled `merge`.
            if offset == 1 {
                stream.push_str(&format!("from :{}\n", MAIN_LEN - 10 + 1));
            } else {
                stream.push_str(&format!("from :{}\n", MAIN_LEN + 1));
            }
            stream.push_str("M 644 inline t.txt\n");
            data(&mut stream, &format!("topic body {offset}\n"));
        }
        // The merge at the tip: `main`'s head and `topic`'s tip.
        let merge_mark = MAIN_LEN + 3;
        stream.push_str(&format!("commit refs/heads/main\nmark :{merge_mark}\n"));
        stream.push_str("author t <t@example.com> 3000 +0000\n");
        stream.push_str("committer t <t@example.com> 3000 +0000\n");
        data(&mut stream, "merge topic into main\n");
        // First parent, then the merged-in one: `from` then `merge`. `main`'s
        // head is the last mark the linear chain claimed — the chain numbers
        // marks 1..=MAIN_LEN, so the topic's two commits start after it.
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
        git(&repo, &["reset", "-q", "--hard", "main"]);
        Long { _tmp: tmp, project }
    })
}

/// The harness over the long history with its Log window open.
///
/// The window is paged through the REAL affordance — the "Load more" button the
/// pane paints, clicked the way a developer clicks it — rather than by priming
/// [`turbogit_app::root_caches::RootCaches`] directly: the point of these tests
/// is what a frame costs while a 150-commit window is being paged in and
/// scrolled, and a primed cache would skip the half of the path where the
/// window grows under the viewport.
fn long_log_harness() -> Harness<'static, AppState> {
    let long = long_history();
    let mut state = AppState::new(long.project.clone());
    assert_eq!(state.multi.roots.len(), 1, "one root discovered");
    let root = state.multi.roots[0].id.clone();
    state.fetch_log(root.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    // The shared shell launcher, at this suite's box. Its preamble drains events
    // every frame, for production parity with `src/app.rs`.
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    settle_until(&mut harness, "c149");
    // Page until the whole history is in the window. Each click is the pane's
    // own affordance; a history of 153 commits needs three more pages after the
    // first.
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
/// egui applies whole in a single frame — a bare wheel delta is spread over
/// several frames, and a list that pages while it is still scrolling reads as
/// two arrivals at the bottom rather than one.
fn wheel(harness: &mut Harness<'static, AppState>, delta: f32) {
    // The pointer only has to be inside the list. Under the search box is
    // inside the list at every scroll position, and that box is painted at
    // every one.
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

/// The two lane colours these tests tell apart, copied from the log's own graph
/// palette (`GRAPH_COLORS` in `log_window.rs`, applied by `root_color`):
/// lane 0 is the blue the first commit in a window takes, lane 1 is the pink a
/// merge OPENS for the branch it merges in. A positional (viewport-scoped) walk
/// would hand any commit that happened to be painted first lane 0's blue.
const LANE_0_BLUE: Color32 = Color32::from_rgb(80, 140, 230);
const LANE_1_PINK: Color32 = Color32::from_rgb(220, 120, 140);

/// The fixture's display indices, newest-first, as the pane orders them by
/// commit time: the merge is row 0, `main`'s nine newest commits are rows 1..=9,
/// the side branch's tip is row 10, `c140` is row 11 and the side branch's root
/// is row 12. `main`'s commits are two seconds apart and the branch's two occupy
/// the free seconds either side of `c140`, which is what puts them there.
const TOPIC_TIP_ROW: usize = 10;
const TOPIC_ROOT_ROW: usize = 12;

/// How many rows the fixture's window holds, for the comment in each test. Read
/// through the pane, not the cache: the "N shown" line IS the window's row count.
fn shown_rows(harness: &Harness<'_, AppState>) -> usize {
    painted_text(harness)
        .into_iter()
        .find_map(|t| t.strip_suffix(" shown")?.parse().ok())
        .expect("the pane's row count line is painted")
}

/// With a long window loaded, the list builds the rows its viewport can hold and
/// no more — the frame's work is a function of the viewport, not of how much
/// history is paged in.
///
/// The bound is derived entirely from painted output, in three steps:
/// 1. the list's viewport height `H` — the clip rect the row nodes were painted
///    under, which is the scroll area's own answer to "how tall am I";
/// 2. the row pitch `p` — the distance between two adjacent rows' painted cell
///    centres, which is `FILE_ROW_HEIGHT` plus the row spacing egui adds;
/// 3. the bound `ceil(H / p) + 2` — the rows that fit in `H` at pitch `p`, plus
///    the one row the scroll area builds past the edge, plus one for the
///    floor/ceiling rounding. Nothing here assumes a row height, a spacing, or a
///    window size: all three are read off the frame being measured.
#[test]
fn a_long_window_builds_only_the_rows_its_viewport_can_hold() {
    let mut harness = long_log_harness();
    let window = shown_rows(&harness);

    // At the top of the list, and then scrolled ten rows down — a known offset,
    // established from the painted output rather than assumed: after the wheel
    // the row at the viewport's top edge is the side branch's tip, which the
    // fixture placed at row 10.
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

        // The window has to be deep enough for the bound to mean anything: at
        // least four viewports of rows, so a list that built a viewport's worth
        // would be building a quarter of what it holds.
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
        // …and the viewport is not merely within the bound but FULL: a list that
        // built three rows because three were cheap would satisfy the bound too.
        assert!(
            built + 2 >= (viewport.height() / pitch).floor() as usize,
            "{label}: only {built} rows for a {:.0}px viewport at a {pitch:.0}px pitch — \
             the bound is not being satisfied by an empty list",
            viewport.height()
        );
    }

    // The offset is known because the painted output says so: ten rows down, the
    // row at the viewport's top edge is the fixture's row 10, the side branch's
    // tip. The scroll was `10 x pitch`, and this is that arithmetic checked
    // against the frame rather than assumed.
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

/// A commit scrolled well outside the viewport paints nothing at all — no
/// lane node, no text, nothing in the frame's output.
///
/// The fixture is shaped so "well outside" is unambiguous. Scrolled ten rows
/// down, the head of the list sits nine rows above the viewport's top edge and
/// the root commit 141 rows below its bottom edge: neither is within a row of
/// the edge, so no one-row overshoot could reach either. Both are IN the
/// window — the pane's own "N shown" line says how deep it is — so what changes
/// between the two observations is the viewport and nothing else.
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

    // Premise: at the top of the list the head of the window paints, and the
    // window's LAST row does not — it is 152 rows below the viewport.
    assert_painted(&harness, head_subject);
    assert!(
        lane_node_colour(&harness, head_subject).is_some(),
        "the head commit's lane node must paint before the scroll"
    );
    assert!(
        text_center_y(&harness, oldest_subject).is_none(),
        "the window's last row is far below the viewport at the top of the list"
    );

    // Nine rows above the viewport: the head of the list.
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
    // 141 rows below the viewport, and still nothing.
    assert!(
        text_center_y(&harness, oldest_subject).is_none(),
        "the window's last row is far below the viewport and must not paint"
    );

    // …and scrolling all the way to the end of the window brings the row that is
    // IN the viewport into the frame — while the one that is not stays out.
    wheel(&mut harness, -100_000.0);
    assert_painted(&harness, oldest_subject);
    assert!(
        text_center_y(&harness, head_subject).is_none(),
        "the head of the list is still far above the viewport at the end of the window"
    );
    let viewport = list_viewport(&harness);
    let rows = built_rows(&harness);
    // The pitch, measured from the frame's own rows rather than assumed, so
    // this bound scales with whatever the row height is.
    let pitch = rows.windows(2).map(|w| w[1].0 - w[0].0).fold(0.0, f32::max);
    // Every row built at the end of the window is one the viewport can show,
    // plus at most the one the scroll area builds past its edge. The end of
    // the window is where that overshoot is largest and least symmetric: the
    // area clamps its offset to `content_height - viewport_height`, and the
    // leftover of a partial row pitch is spent ABOVE the viewport's top edge
    // while the last row sits flush against its bottom. So the bound is
    // stated in rows, not as a hard-coded pixel slack: a fixed slack has to
    // be re-tuned whenever the pane's height changes, and the 48px the shell
    // gave back to the tool window when the repo header was deleted was
    // enough to tip one over.
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
    // …and the list built one viewport of rows, not a window's worth of them.
    let fits = (viewport.height() / pitch).ceil() as usize;
    assert!(
        rows.len() <= fits + 1,
        "the list built {} rows for a {:.0}px viewport at a {pitch:.0}px pitch \
         — more than one viewport's worth",
        rows.len(),
        viewport.height()
    );
}

/// A commit's lane node is the same colour with the list scrolled far away and
/// scrolled back — the regression guard for walking lanes over the WHOLE loaded
/// window rather than the drawn rows.
///
/// **Why this commit.** `topic root` is the side branch's root: it is reached
/// only through the merge at row 0, which OPENS a second lane for the branch it
/// merges in. Nothing about the commit's own position decides that. A walk
/// scoped to the painted rows would give it lane 0 — the blue every mainline
/// commit has — the moment the viewport no longer contains the merge, because a
/// contiguous slice of a linear history all shares lane 0. So if the walk were
/// scoped to the viewport, this commit would change colour as the list scrolled,
/// and the two observations below would disagree.
///
/// **Non-vacuity.** The assertions below are not satisfied by a walk that
/// happens to agree: at the second offset this commit is the FIRST row of the
/// viewport — the one position where a positional walk is unambiguous — and it
/// still paints lane 1's pink, while `c149` on `main` paints lane 0's blue in
/// the same frame. The two colours are asserted to be different, so a test that
/// read the wrong lane, or a walk that handed every row lane 0, would fail.
#[test]
fn a_side_branch_commit_keeps_its_lane_colour_across_scrolling() {
    let mut harness = long_log_harness();
    let mainline = "c149";
    let tip = "topic tip";
    let side = "topic root";

    // At the top of the list the window's first row is the merge, so the walk
    // sees the whole branch: the side lane exists and both commits on it are
    // pink while `main` is blue.
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

    // Scrolled so the side branch's root is the FIRST row of the viewport and
    // the merge is nine rows above it — the position a viewport-scoped walk
    // cannot get right.
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

    // Far away, then back: the same commit, the same colour.
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
