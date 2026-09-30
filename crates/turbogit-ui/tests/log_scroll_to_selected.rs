//! Log-view-scaling 08 — selecting a commit from outside the list scrolls it
//! into view.
//!
//! Every test here drives a REAL caller to a selection — a click on the details
//! pane's parent link, a click on a blame row, a verb chosen on the commit
//! context menu — and then asserts on PAINTED OUTPUT: that the selected commit's
//! row is on screen. It cannot assert on a row's accessibility node before the
//! scroll, because a row outside the viewport was never built and has no node —
//! which is the whole reason this feature exists.

use std::path::{Path, PathBuf};

use egui::Shape;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::harness::{painted_text, shell_harness_over};
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};

// --- painted-output helpers ---------------------------------------------------

/// Assert some painted text galley is EXACTLY `needle`. Not
/// `test_support::harness::assert_painted`, which matches with `t.contains`:
/// every `needle` here is a whole commit subject or a full row label, so
/// containment would let a longer galley that merely mentions it pass.
#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == needle),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Step frames until the painted output settles AND no keyed read is pending.
///
/// The read gate is the load-bearing half: the blame read is a real `git blame`
/// subprocess, so a frame-count settle returns before it lands and the blame
/// view paints an empty shell. This is the same rule `blame_view.rs` and
/// `diff_viewer.rs` settle by.
///
/// Not either shared settle: both key their fingerprint on painted text ALONE, but the
/// `blame` and `log` reads a parent-link click triggers land their events after the
/// pane has stopped changing — so a text-only fingerprint reports the list settled and
/// the test measures a viewport the user never saw.
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
    panic!("log/blame layout did not settle within 15s");
}

/// A painted text's vertical centre — every cell of a commit row is centred on
/// its row, so this is how a test finds a row's y without asking the row model.
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

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// A `git` runner that pins the commit identity on every invocation, and deliberately
/// NOT `test_support::git_seed::git`, which takes no per-call env: these rows are
/// matched by subject and hash, so a machine-dependent identity moves the labels.
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

const COMMITS: usize = 2 * LOG_BATCH_SIZE;

struct Seed {
    _tmp: tempfile::TempDir,
    project: PathBuf,
    repo: PathBuf,
}

fn seeded() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).expect("alpha dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "t"]);
    // One fast-import batch rather than one subprocess per commit.
    //
    // Two files, for two different callers:
    // - `f.txt` is rewritten by EVERY commit, so every row in the log has a
    //   changed-file entry and the parent-link test's panes have something to
    //   describe;
    // - `acc.txt` only ever gains a line, and only every fifth commit. At HEAD
    //   it holds 20 lines attributed to 20 different commits spread across the
    //   whole history, so blaming it yields rows whose commits are far apart —
    //   which is what makes the blame caller a "far outside the drawn rows" case.
    let mut acc = String::new();
    let mut stream = String::new();
    let data = |stream: &mut String, s: &str| {
        stream.push_str(&format!("data {}\n{s}", s.len()));
    };
    acc.push_str("acc line 0\n");
    stream.push_str("commit refs/heads/main\nmark :1\n");
    stream.push_str("author t <t@example.com> 1000 +0000\n");
    stream.push_str("committer t <t@example.com> 1000 +0000\n");
    data(&mut stream, "c0\n");
    stream.push_str("M 644 inline f.txt\n");
    data(&mut stream, "f line 0\n");
    stream.push_str("M 644 inline acc.txt\n");
    data(&mut stream, &acc);
    for i in 1..COMMITS {
        stream.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
        stream.push_str(&format!(
            "author t <t@example.com> {} +0000\n",
            1000 + i as i64
        ));
        stream.push_str(&format!(
            "committer t <t@example.com> {} +0000\n",
            1000 + i as i64
        ));
        data(&mut stream, &format!("c{i}\n"));
        stream.push_str(&format!("from :{i}\n"));
        let f = format!("f line {i}\n");
        stream.push_str("M 644 inline f.txt\n");
        data(&mut stream, &f);
        if i % 5 == 0 {
            acc.push_str(&format!("acc line {i}\n"));
            stream.push_str("M 644 inline acc.txt\n");
            data(&mut stream, &acc);
        }
    }
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
    Seed {
        _tmp: tmp,
        project,
        repo,
    }
}

/// The full hash of the commit whose subject is `subject` — the fixture's
/// subjects are unique, which is what lets a test name a row by what it paints
/// and still click it.
fn hash_of(repo: &Path, subject: &str) -> String {
    git(repo, &["log", "--format=%H %s"])
        .lines()
        .find_map(|line| {
            let (hash, rest) = line.split_once(' ')?;
            (rest.trim() == subject).then(|| hash.to_owned())
        })
        .unwrap_or_else(|| panic!("no commit with subject {subject}"))
}

/// The row label `commit_row` publishes: `<short hash> <subject>`.
fn row_label(repo: &Path, subject: &str) -> String {
    format!("{} {}", short(&hash_of(repo, subject)), subject)
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 1, "one root discovered");
    let root = state.multi.roots[0].id.clone();
    state.fetch_log(root);
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    // Page the whole history in through the pane's own affordance, so the
    // window is as deep as the real one gets.
    for _ in 0..4 {
        if harness.query_by_label("Load more").is_none() {
            break;
        }
        harness.get_by_label("Load more").click();
        settle(&mut harness);
    }
    settle(&mut harness);
    harness
}

/// Scroll the commit list by `delta` pixels with one wheel gesture. The pointer
/// only has to be inside the list, and the search box above it is painted at
/// every scroll position.
fn wheel(harness: &mut Harness<'static, AppState>, delta: f32) {
    wheel_hovering(harness, delta, 60.0)
}

/// [`wheel`] with the hover depth below the search box stated, for when a popup
/// is open over the top of the list: the wheel belongs to whatever is under the
/// pointer, and a menu anchored near the first row covers the shallow depths.
fn wheel_hovering(harness: &mut Harness<'static, AppState>, delta: f32, below_toolbar: f32) {
    let toolbar = harness.get_by_label("Search commits").rect();
    harness.hover_at(egui::pos2(
        toolbar.center().x,
        toolbar.bottom() + below_toolbar,
    ));
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

/// The list's own top and bottom edges, from the chrome it paints around its
/// rows: the micro-headers above and the "N shown" line below. A row is on
/// screen when its band sits between them.
fn list_edges(harness: &Harness<'_, AppState>) -> (f32, f32) {
    let header = text_center_y(harness, "HASH").expect("the graph's micro-headers paint");
    let count = painted_text(harness)
        .into_iter()
        .find(|t| t.ends_with(" shown"))
        .expect("the pane's row count line paints");
    let footer = text_center_y(harness, &count).expect("the row count line is painted");
    (header + 6.0, footer - 8.0)
}

/// The row rectangle the list painted for `commit`, from the accessibility node
/// `commit_row` publishes.
///
/// The node's rect and nothing else: a commit's SUBJECT is painted in the
/// details pane too once the commit is selected, so a probe that looked the text
/// up by content would find the wrong element the moment a row was selected.
fn row_rect(harness: &Harness<'_, AppState>, repo: &Path, commit: usize) -> Option<egui::Rect> {
    let label = row_label(repo, &subject_at(commit));
    harness.query_by_label(&label).map(|node| node.rect())
}

/// The commits whose rows are FULLY on screen, in display order.
///
/// "Painted" is not the same as "on screen": the scroll area lays out one row
/// past the bottom edge, so a row can be in the frame while its band is below
/// the fold — which is also why such a row cannot be clicked, because the
/// pointer at its centre lands on whatever is painted there instead. This is
/// therefore the set a user can see AND press.
fn visible_commits(harness: &Harness<'_, AppState>, repo: &Path) -> Vec<(usize, String)> {
    let (top, bottom) = list_edges(harness);
    let mut rows: Vec<(f32, usize, String)> = (0..COMMITS)
        .filter_map(|commit| {
            let rect = row_rect(harness, repo, commit)?;
            (rect.top() >= top && rect.bottom() <= bottom).then_some((
                rect.top(),
                commit,
                subject_at(commit),
            ))
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter()
        .map(|(_, commit, subject)| (commit, subject))
        .collect()
}

/// Whether `commit`'s row is fully on screen — the property these tests assert,
/// as opposed to "in the frame", which one row past the fold also satisfies.
fn is_visible(harness: &Harness<'_, AppState>, repo: &Path, commit: usize) -> bool {
    let (top, bottom) = list_edges(harness);
    row_rect(harness, repo, commit).is_some_and(|rect| rect.top() >= top && rect.bottom() <= bottom)
}

/// A commit's subject: the fixture numbers its commits, and the window is
/// newest-first, so a LOWER number is a row further DOWN the list.
fn subject_at(commit: usize) -> String {
    format!("c{commit}")
}

/// Following a commit back through history shows the commit. The parent link in
/// the commit details pane is the first of the three callers: the link is
/// clicked, and the parent — the row BELOW the one just clicked, which the list
/// did not build — has to be on screen afterwards.
///
/// The fixture is arranged so the parent really is off screen: the list is
/// scrolled to its end and then up a little, so the last row it paints has a
/// parent, and that parent is many rows below the fold. The test asserts that
/// premise on the frame before the click, so it cannot pass by accident because
/// the parent happened to be visible.
#[test]
fn clicking_a_parent_link_scrolls_that_parents_row_into_view() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

    // Park the list near its end, then scroll up far enough that the bottom edge
    // is in the middle of the history.
    wheel(&mut harness, -100_000.0);
    wheel(&mut harness, 600.0);
    let (number, subject) = visible_commits(&harness, &seed.repo)
        .pop()
        .expect("the list is showing rows");
    let parent = subject_at(number - 1);
    assert!(
        number > 0,
        "the premise: the bottom visible row must have a parent in the window"
    );
    assert!(
        !is_visible(&harness, &seed.repo, number - 1),
        "the premise: `{parent}` must be off screen before the link is clicked"
    );

    // Click the bottom row on screen — a real click on its accessibility label.
    harness
        .get_by_label(&row_label(&seed.repo, &subject))
        .click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(hash_of(&seed.repo, &subject).as_str()),
        "the row click selected the commit the test clicked"
    );

    // Click its parent link in the details pane: a real click on the link, whose
    // label is the parent's short hash.
    harness
        .get_by_label(&short(&hash_of(&seed.repo, &parent)))
        .click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(hash_of(&seed.repo, &parent).as_str()),
        "the link selected the parent"
    );
    assert!(
        is_visible(&harness, &seed.repo, number - 1),
        "`{parent}` was off screen and the link must have scrolled it into view"
    );
    // …and the rest of the parent link's behaviour is intact: the changed-file
    // selection is dropped, and both right-hand panes now describe the parent.
    assert_eq!(
        harness.state().ui.log_selected_file,
        None,
        "following history must not carry the previous commit's changed file over"
    );
    // …and both right-hand panes now describe the PARENT, not the commit that
    // was selected a moment ago: the details pane leads with the parent's hash
    // chip and subject, and the changed-files list is the parent's.
    assert_painted(&harness, &parent);
    assert_painted(&harness, &short(&hash_of(&seed.repo, &parent)));
    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "f.txt");
}

/// A row in the blame view brings that commit into view in the log — the second
/// of the three callers, and the one that can be FAR outside the drawn rows.
///
/// `acc.txt` only ever gains a line, and only every fifth commit, so at HEAD it
/// holds 20 lines attributed to 20 different commits spread across the whole
/// 100-commit history. The first of those lines belongs to the root commit,
/// which sits 99 rows down the log — far outside anything the list has built
/// when the list is at its top. So this is the "far outside the drawn rows" case
/// the ticket asks for, reached by one click on a surface that has no list of
/// its own in view.
#[test]
fn clicking_a_blame_row_brings_that_commit_into_view_in_the_log() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

    // Select the newest commit that touched `acc.txt` (every fifth one), land on
    // its changed-file entry, and blame it through the log's own context menu.
    let touching = (0..COMMITS)
        .rev()
        .find(|i| i % 5 == 0)
        .expect("acc.txt commits");
    harness
        .get_by_label(&row_label(&seed.repo, &subject_at(touching)))
        .click();
    settle(&mut harness);
    harness.get_by_label("acc.txt").click();
    settle(&mut harness);
    // The premise, checked while the log is still the painted surface: the root
    // commit is 99 rows down a list sitting at its top, so it is nowhere near
    // being on screen.
    assert!(
        !is_visible(&harness, &seed.repo, 0),
        "the root commit must start far outside the list's rows"
    );
    test_support::harness::right_click_row(&mut harness, "acc.txt");
    test_support::harness::click_menu_item(&mut harness, "Show blame", "Show blame");
    settle(&mut harness);
    assert!(
        harness.state().ui.blame.is_some(),
        "the blame view is open, which is the premise of this test"
    );

    // `acc.txt`'s first line is attributed to the root commit.
    let oldest = subject_at(0);
    let blame_row = format!("{} acc line 0", short(&hash_of(&seed.repo, &oldest)));
    assert!(
        harness.query_by_label(&blame_row).is_some(),
        "the blame view must attribute the file's first line to the root commit; painted:\n{:#?}",
        painted_text(&harness)
    );

    // Click it — a real click on the blame row's own accessibility label.
    harness.get_by_label(&blame_row).click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.blame,
        None,
        "clicking a blame row closes the view, so the log is what the user sees next"
    );
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(hash_of(&seed.repo, &oldest).as_str()),
        "the blame row selected its commit"
    );
    assert!(
        is_visible(&harness, &seed.repo, 0),
        "the commit was chosen from a surface with no list in view, so the log must have \
         scrolled the whole way down to show it"
    );
}

/// Choosing a commit through the commit context menu leaves that commit visible
/// in the list — the third caller.
///
/// The sequence a user actually performs: right-click a row to open the menu on
/// it, scroll the list away while the menu stays open (the menu is app state,
/// not a frame's local, so it survives), then choose a verb. Every verb acts on
/// the commit the menu was opened on, so the list is asked to show it again —
/// otherwise the verb confirms "this commit" while the list has moved on to
/// somewhere else entirely.
///
/// The verb driven is **Copy hash**: it is the one that leaves the commit
/// selected and the log list visible, with no dialog over it, so the assertion
/// can be about the list rather than about a modal. (`CopyMessage` would do as
/// well; the dialog-driven verbs — cherry-pick, reword, drop — put a surface
/// between the user and the list, and what they do to the selection afterwards
/// belongs to their own tickets.)
#[test]
fn choosing_a_verb_on_the_commit_menu_leaves_that_commit_visible() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

    // Open the commit menu on HEAD — the first row of the list — then scroll the
    // list away while the menu stays open.
    let target = subject_at(COMMITS - 1);
    test_support::harness::right_click_row(&mut harness, &target);
    settle(&mut harness);
    assert!(
        harness.state().ui.log_commit_menu.is_some(),
        "the right-click opened the commit menu"
    );
    assert!(
        is_visible(&harness, &seed.repo, COMMITS - 1),
        "the premise: the menu's commit starts on screen"
    );
    // Hover low in the list so the wheel belongs to the list and not to the open
    // menu, which is anchored over the top of it.
    wheel_hovering(&mut harness, -1_000.0, 520.0);
    assert!(
        !is_visible(&harness, &seed.repo, COMMITS - 1),
        "the premise: the menu's commit is now off screen while its menu is open"
    );

    // Choose a verb on the still-open menu.
    test_support::harness::click_menu_item(&mut harness, "Copy hash", "Copy hash");
    // The verb lands on the last frame the click stepped, and the list consumes
    // the request on the NEXT one — and a scroll area applies the new offset when
    // the frame after THAT begins. So three settles, not one: reading the paint
    // straight after the click reads the list as it was.
    settle(&mut harness);
    settle(&mut harness);
    settle(&mut harness);
    assert!(
        harness.state().ui.log_commit_menu.is_none(),
        "choosing a verb closes the menu"
    );
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(hash_of(&seed.repo, &target).as_str()),
        "the verb acted on the commit its menu was opened on"
    );
    assert!(
        is_visible(&harness, &seed.repo, COMMITS - 1),
        "the list was scrolled away from the commit whose verb was chosen, and the verb \\
         must have brought it back into view"
    );
}

/// Selecting a commit that is ALREADY on screen does not yank the list.
///
/// The arrangement matters: the list is scrolled into the middle of the history
/// first, because a list already at its top cannot demonstrate anything — asking
/// it to re-centre a row near its top edge is a no-op the clamp hides. Mid-list,
/// a yank moves the top row by half a viewport, which is what this asserts.
///
/// The interaction is the real one: a commit row is clicked, and then its parent
/// link in the details pane — a selection made from OUTSIDE the list, of a commit
/// that is plainly on screen.
#[test]
fn selecting_a_commit_already_in_view_does_not_yank_the_list() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

    // Into the middle of the history, so both the row and its parent sit well
    // inside the viewport with room to move in either direction.
    wheel(&mut harness, -1_500.0);
    let on_screen = visible_commits(&harness, &seed.repo);
    assert!(
        on_screen.len() > 10,
        "the premise: the list is showing rows, got {}",
        on_screen.len()
    );
    let (number, subject) = on_screen[on_screen.len() / 2].clone();
    let parent = number - 1;
    assert!(
        is_visible(&harness, &seed.repo, parent),
        "the premise: the parent is on screen too, so the link has no reason to scroll"
    );

    harness
        .get_by_label(&row_label(&seed.repo, &subject))
        .click();
    settle(&mut harness);
    let top_before = visible_commits(&harness, &seed.repo)
        .first()
        .map(|(commit, _)| *commit)
        .expect("rows are on screen");

    // Now select the parent from the details pane — outside the list.
    harness
        .get_by_label(&short(&hash_of(&seed.repo, &subject_at(parent))))
        .click();
    settle(&mut harness);
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(hash_of(&seed.repo, &subject_at(parent)).as_str()),
        "the link selected the parent"
    );
    assert!(
        is_visible(&harness, &seed.repo, parent),
        "the parent is still on screen"
    );
    assert_eq!(
        visible_commits(&harness, &seed.repo)
            .first()
            .map(|(commit, _)| *commit),
        Some(top_before),
        "a commit that was already in view must not move the list"
    );
}

/// A scroll request is answered ONCE: the list settles where the selection put
/// it, the request is consumed, and the frame stops asking to be repainted.
///
/// The repaint half is measured rather than asserted by faith:
/// `Harness::try_run` steps frames until the UI stops asking to be repainted and
/// FAILS when it never does, which is exactly what a list still being told to
/// scroll on every frame looks like.
#[test]
fn a_scroll_request_is_answered_once_and_then_the_list_settles() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

    // Park near the end, then scroll up so the bottom edge is mid-history: the
    // link's parent is far enough below the fold that the scroll has real work.
    wheel(&mut harness, -100_000.0);
    wheel(&mut harness, 600.0);
    let (number, subject) = visible_commits(&harness, &seed.repo)
        .pop()
        .expect("rows are on screen");
    let parent = number - 1;
    assert!(
        !is_visible(&harness, &seed.repo, parent),
        "the premise: the parent is off screen, so the link has work to do"
    );

    harness
        .get_by_label(&row_label(&seed.repo, &subject))
        .click();
    settle(&mut harness);
    harness
        .get_by_label(&short(&hash_of(&seed.repo, &subject_at(parent))))
        .click();
    // The link lands on one frame, the list consumes the request on the next, and
    // the area applies the offset on the one after that — three settles, so the
    // paint being read is the settled one.
    settle(&mut harness);
    settle(&mut harness);
    settle(&mut harness);

    assert!(
        is_visible(&harness, &seed.repo, parent),
        "the link scrolled the parent into view"
    );
    assert_eq!(
        harness.state().ui.log_scroll_to,
        None,
        "the request is consumed, so it cannot be re-issued on a later frame"
    );

    // Nothing moves the list over the next twenty frames…
    let settled_top = visible_commits(&harness, &seed.repo)
        .first()
        .map(|(commit, _)| *commit);
    for _ in 0..20 {
        harness.step();
    }
    assert_eq!(
        visible_commits(&harness, &seed.repo)
            .first()
            .map(|(commit, _)| *commit),
        settled_top,
        "the list kept moving after the selection settled"
    );
    assert_eq!(
        harness.state().ui.log_scroll_to,
        None,
        "the request was re-armed"
    );
    // …and the UI comes to rest rather than being driven frame after frame.
    if let Err(err) = harness.try_run() {
        panic!("the list never stopped asking to be repainted: {err:?}");
    }
}
