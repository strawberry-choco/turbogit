use std::path::{Path, PathBuf};

use egui::Shape;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{painted_text, shell_harness_over};
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == needle),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

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
    git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(
        &repo,
        &["config", "user.email", "t@example.com"],
        &GitEnv::fixture(),
    );
    git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
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
    git_with_env(
        &repo,
        &["reset", "-q", "--hard", "main"],
        &GitEnv::fixture(),
    );
    Seed {
        _tmp: tmp,
        project,
        repo,
    }
}

fn hash_of(repo: &Path, subject: &str) -> String {
    git_with_env(repo, &["log", "--format=%H %s"], &GitEnv::fixture())
        .lines()
        .find_map(|line| {
            let (hash, rest) = line.split_once(' ')?;
            (rest.trim() == subject).then(|| hash.to_owned())
        })
        .unwrap_or_else(|| panic!("no commit with subject {subject}"))
}

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

fn wheel(harness: &mut Harness<'static, AppState>, delta: f32) {
    wheel_hovering(harness, delta, 60.0)
}

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

fn list_edges(harness: &Harness<'_, AppState>) -> (f32, f32) {
    let header = text_center_y(harness, "HASH").expect("the graph's micro-headers paint");
    let count = painted_text(harness)
        .into_iter()
        .find(|t| t.ends_with(" shown"))
        .expect("the pane's row count line paints");
    let footer = text_center_y(harness, &count).expect("the row count line is painted");
    (header + 6.0, footer - 8.0)
}

fn row_rect(harness: &Harness<'_, AppState>, repo: &Path, commit: usize) -> Option<egui::Rect> {
    let label = row_label(repo, &subject_at(commit));
    harness.query_by_label(&label).map(|node| node.rect())
}

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

fn is_visible(harness: &Harness<'_, AppState>, repo: &Path, commit: usize) -> bool {
    let (top, bottom) = list_edges(harness);
    row_rect(harness, repo, commit).is_some_and(|rect| rect.top() >= top && rect.bottom() <= bottom)
}

fn subject_at(commit: usize) -> String {
    format!("c{commit}")
}

#[test]
fn clicking_a_parent_link_scrolls_that_parents_row_into_view() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

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

    harness
        .get_by_label(&row_label(&seed.repo, &subject))
        .click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(hash_of(&seed.repo, &subject).as_str()),
        "the row click selected the commit the test clicked"
    );

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
    assert_eq!(
        harness.state().ui.log_selected_file,
        None,
        "following history must not carry the previous commit's changed file over"
    );
    assert_painted(&harness, &parent);
    assert_painted(&harness, &short(&hash_of(&seed.repo, &parent)));
    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "f.txt");
}

#[test]
fn clicking_a_blame_row_brings_that_commit_into_view_in_the_log() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

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

    let oldest = subject_at(0);
    let blame_row = format!("{} acc line 0", short(&hash_of(&seed.repo, &oldest)));
    assert!(
        harness.query_by_label(&blame_row).is_some(),
        "the blame view must attribute the file's first line to the root commit; painted:\n{:#?}",
        painted_text(&harness)
    );

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

#[test]
fn choosing_a_verb_on_the_commit_menu_leaves_that_commit_visible() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

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
    wheel_hovering(&mut harness, -1_000.0, 520.0);
    assert!(
        !is_visible(&harness, &seed.repo, COMMITS - 1),
        "the premise: the menu's commit is now off screen while its menu is open"
    );

    test_support::harness::click_menu_item(&mut harness, "Copy hash", "Copy hash");
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

#[test]
fn selecting_a_commit_already_in_view_does_not_yank_the_list() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

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

#[test]
fn a_scroll_request_is_answered_once_and_then_the_list_settles() {
    let seed = seeded();
    let mut harness = log_harness(&seed);

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
    if let Err(err) = harness.try_run() {
        panic!("the list never stopped asking to be repainted: {err:?}");
    }
}
