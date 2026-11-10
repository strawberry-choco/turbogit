use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;
use test_support::git_seed::{git, repo_with_one_commit, repo_with_origin};
use test_support::harness::{
    assert_painted, filled_circles, filled_rects, painted_galleys, painted_text,
    repo_shell_harness, settle_quiet, settle_reads, shell_harness_over,
};
use turbogit_app::log_filters::{DateFilter, Fold, RefGroup, RootFilter};
use turbogit_app::root_caches::{LogRead, LogScope};
use turbogit_app::state::{AppState, Dialog, NewBranchBase, Tab};
use turbogit_domain::model::RootId;

fn log_harness() -> Harness<'static, AppState> {
    let (mut harness, _tmp) = repo_shell_harness();
    harness.state_mut().ui.tab = Tab::Log;
    settle_quiet(&mut harness);
    harness
}

#[test]
fn the_commits_pane_paints_the_four_intellij_filter_dropdowns() {
    let harness = log_harness();

    for label in ["Branch", "User", "Date", "Paths", "Clear"] {
        harness.get_by_label(label);
    }
    assert_painted(&harness, "Branch");
    assert_painted(&harness, "User");
    assert_painted(&harness, "Date");
    assert_painted(&harness, "Paths");
}

#[test]
fn a_date_preset_changes_the_button_and_asks_for_a_composite_read() {
    let mut harness = log_harness();
    harness.get_by_label("Date").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Last 7 days").click();
    settle_quiet(&mut harness);

    assert!(matches!(
        harness.state().ui.log_date_filter,
        DateFilter::Last7Days { .. }
    ));
    let key = harness
        .state()
        .log_filter_key()
        .expect("a date preset must ask git for a filtered window");
    assert!(key.since.is_some());
    assert_eq!(key.until, None);
    assert_eq!(key.author, None);
    assert_painted(&harness, "Date: Last 7 days");
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap()
}

fn local_wall(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(h, min, s)
        .unwrap()
}

fn utc_of_wall(wall: NaiveDateTime) -> DateTime<Utc> {
    Local
        .from_local_datetime(&wall)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&wall))
        .with_timezone(&Utc)
}

#[test]
fn a_picked_day_sets_the_after_bound_and_asks_git_for_unix_seconds() {
    let mut harness = log_harness();
    harness.state_mut().ui.log_date_filter =
        DateFilter::from_bounds(Some(utc_of_wall(local_wall(2026, 11, 7, 9, 30, 0))), None);
    settle_quiet(&mut harness);

    harness.get_by_label("Date").click();
    settle_quiet(&mut harness);
    harness.get_by_label("2026-11-07").click();
    settle_quiet(&mut harness);
    harness.get_by_label("8").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Save").click();
    settle_quiet(&mut harness);

    let key = harness
        .state()
        .log_filter_key()
        .expect("a picked day must ask git for a filtered window");
    assert_eq!(key.until, None);
    let since_wall = Utc
        .timestamp_opt(key.since.expect("a lower bound"), 0)
        .single()
        .unwrap()
        .with_timezone(&Local)
        .naive_local();
    assert_eq!(since_wall, local_wall(2026, 11, 8, 9, 30, 0));
    assert_painted(&harness, "Date: Since 2026-11-08 09:30");
}

#[test]
fn clearing_one_bound_leaves_the_other_open() {
    let mut harness = log_harness();
    let after = at(2026, 11, 7, 9, 30, 0);
    harness.state_mut().ui.log_date_filter =
        DateFilter::from_bounds(Some(after), Some(at(2026, 11, 8, 23, 59, 59)));
    settle_quiet(&mut harness);

    harness.get_by_label("Date").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Clear before").click();
    settle_quiet(&mut harness);

    assert_eq!(harness.state().ui.log_date_filter.before(), None);
    assert_eq!(harness.state().ui.log_date_filter.after(), Some(after));
    let key = harness
        .state()
        .log_filter_key()
        .expect("the surviving bound must still ask git for a window");
    assert_eq!(key.until, None);
}

#[test]
fn the_date_popup_has_no_manual_date_input() {
    let mut harness = log_harness();
    harness.get_by_label("Date").click();
    settle_quiet(&mut harness);

    assert_not_painted(&harness, "YYYY-MM-DD");
    assert_not_painted(&harness, "Use YYYY-MM-DD");
}

#[test]
fn a_user_filter_reads_only_matching_commits_from_the_real_repository() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let work: PathBuf = repo_with_one_commit(&project, "alpha");
    git(
        &work,
        &[
            "-c",
            "user.name=Alice",
            "-c",
            "user.email=alice@example.com",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "alice work",
        ],
    );

    let mut state = AppState::new(project);
    state.ui.tab = Tab::Log;
    let mut harness = shell_harness_over(state, egui::vec2(1024.0, 768.0));
    settle_reads(&mut harness);

    harness.state_mut().ui.log_author_filter = Some("Alice".to_owned());
    settle_reads(&mut harness);

    let key = harness
        .state()
        .log_filter_key()
        .expect("the author filter must build a composite key");
    assert_eq!(key.author.as_deref(), Some("Alice"));
    let root = harness.state().multi.roots[0].id.clone();
    let rows = harness
        .state()
        .caches
        .combined_log(&root, &key)
        .expect("the composite window must have been read");
    assert_eq!(rows.len(), 1, "only Alice's commit may survive");
    assert!(rows[0].message.contains("alice work"));
    assert_eq!(rows[0].author.name, "Alice");
    assert_painted(&harness, "User: Alice");
}

#[test]
fn two_legacy_filters_are_read_together_instead_of_one_silently_winning() {
    let mut harness = log_harness();
    let root = harness.state().multi.roots[0].id.clone();
    {
        let state = harness.state_mut();
        state.ui.log_ref_scope = Some("main".to_owned());
        state.ui.log_path_scope = Some((root, "README.md".into()));
    }

    let key = harness
        .state()
        .log_filter_key()
        .expect("branch and path must be read as one composite window");
    assert_eq!(key.branch.as_deref(), Some("main"));
    assert_eq!(key.path, Some(PathBuf::from("README.md")));
    assert_eq!(harness.state().log_filter_key().unwrap().author, None);
}

#[test]
fn clear_resets_every_filter_including_the_root_selection() {
    let mut harness = log_harness();
    let root = harness.state().multi.roots[0].id.clone();
    {
        let state = harness.state_mut();
        state.ui.log_author_filter = Some("alice".to_owned());
        state.ui.log_date_filter = DateFilter::Last24Hours { since: 1 };
        state.ui.log_path_scope = Some((root, "src/lib.rs".into()));
        state.ui.log_roots = RootFilter::Only(
            state
                .multi
                .roots
                .iter()
                .skip(1)
                .map(|r| r.id.clone())
                .collect(),
        );
    }
    settle_quiet(&mut harness);

    harness.get_by_label("Clear").click();
    settle_quiet(&mut harness);

    let state = harness.state();
    assert_eq!(state.ui.log_author_filter, None);
    assert!(state.ui.log_date_filter.is_all());
    assert_eq!(state.ui.log_path_scope, None);
    assert!(
        state.ui.log_roots.is_all(),
        "Clear means back to rest for the whole filter set, the checked set included"
    );
    assert_eq!(state.log_filter_key(), None);
}

const LOCAL_BAND: &str = "Local branches";
const REMOTE_BAND: &str = "Remote branches";
const TAG_BAND: &str = "Tag refs";
const ALL_REPOS: &str = "All repositories";

/// A shell over real seeded repositories, on the Commits pane.
///
/// `for_roots` rather than `launch_in` because it registers exactly the roots handed to
/// it: a project dir holding a bare `alpha.git` beside `alpha/` would otherwise discover
/// the bare repository as a second root and paint a second set of rows.
fn seeded_log_harness(project: &Path, roots: &[PathBuf]) -> Harness<'static, AppState> {
    let mut state = AppState::for_roots(project, roots);
    state.ui.tab = Tab::Log;
    let mut harness = shell_harness_over(state, egui::vec2(1600.0, 1000.0));
    settle_quiet(&mut harness);
    harness
}

fn root_id(state: &AppState, name: &str) -> RootId {
    state
        .multi
        .roots
        .iter()
        .find(|root| root.id.name() == name)
        .unwrap_or_else(|| panic!("no repository named {name} is registered"))
        .id
        .clone()
}

#[track_caller]
fn open_branch_dropdown(harness: &mut Harness<'static, AppState>) {
    harness.get_by_label("Branch").click();
    settle_quiet(harness);
}

#[track_caller]
fn open_paths_dropdown(harness: &mut Harness<'static, AppState>) {
    harness.get_by_label("Paths").click();
    settle_quiet(harness);
}

/// One primary click at `pos`, as a pointer would make it: hover, then press and release.
/// kittest's own `Node::click` only ever aims at a node's centre, so a target that is not
/// a node, or a node whose centre is swallowed, needs its point named here.
#[track_caller]
fn click_at(harness: &mut Harness<'static, AppState>, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    settle_quiet(harness);
}

/// Reveal a folded band by clicking the centre of its header title.
#[track_caller]
fn reveal_band(harness: &mut Harness<'static, AppState>, band: &str) {
    let centre = harness.get_by_label(band).rect().center();
    click_at(harness, centre);
}

/// Open the Branch dropdown with `groups` already revealed.
///
/// This is the popup's own fold memory, the state a user reaches by clicking a header
/// earlier in the session; writing it here is the fixture, not the assertion. The pick
/// tests start from it rather than from a header click in the same popup because that
/// sequence measured nondeterministic: rows present in the popup's first frame answered
/// every time, rows that grow into the list mid-session did not.
#[track_caller]
fn open_dropdown_with_groups(harness: &mut Harness<'static, AppState>, groups: &[RefGroup]) {
    for group in groups {
        harness
            .state_mut()
            .ui
            .log_ref_folds
            .insert(*group, Fold::Open);
    }
    open_branch_dropdown(harness);
}

/// Nodes of the open popup carrying `label`.
///
/// The left BRANCHES tree paints the very same ref and root names as the dropdown's
/// rows, and `get_by_label` panics rather than arbitrate two hits. Every strip inside
/// one popup shares the left edge of its anchor row; a tree row sits hundreds of pixels
/// away from the popup's own x.
#[track_caller]
fn popup_nodes(harness: &Harness<'static, AppState>, anchor: &str, label: &str) -> usize {
    let left = harness.get_by_label(anchor).rect().min.x;
    harness
        .query_all_by_label(label)
        .filter(|row| (row.rect().min.x - left).abs() < 2.0)
        .count()
}

#[track_caller]
fn click_popup_row(harness: &mut Harness<'static, AppState>, anchor: &str, label: &str) {
    let left = harness.get_by_label(anchor).rect().min.x;
    let mut rows: Vec<_> = harness
        .query_all_by_label(label)
        .filter(|row| (row.rect().min.x - left).abs() < 2.0)
        .collect();
    rows.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    let row = rows.into_iter().next().unwrap_or_else(|| {
        panic!(
            "no row labelled {label:?} inside the open popup anchored by {anchor:?}; \
             painted: {:?}",
            painted_text(harness)
        )
    });
    row.click();
    settle_quiet(harness);
}

#[track_caller]
fn assert_scope_read_settled(harness: &Harness<'static, AppState>, root: &RootId, rev: &str) {
    let read = LogRead::Scope(root.clone(), LogScope::Ref(rev.to_owned()));
    assert!(
        !harness.state().caches.has_log_failure(&read),
        "the scoped read for `{rev}` recorded a failure: {:?}",
        harness.state().caches.log_failure(&read)
    );
    assert!(
        harness.state().caches.ref_log(root, rev).is_some(),
        "the read asked for `{rev}` never answered: a scope whose rev git cannot resolve \
         leaves the cache with no window under that key"
    );
}

fn repo_with_topic_work(project: &Path, name: &str, subject: &str) -> PathBuf {
    let work = repo_with_one_commit(project, name);
    git(work.as_path(), &["checkout", "-q", "-b", "topic"]);
    git(
        work.as_path(),
        &["commit", "-q", "--allow-empty", "-m", subject],
    );
    work
}

#[test]
fn the_paths_dropdown_ticks_repositories_and_stays_open() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha_repo = repo_with_one_commit(&project, "alpha");
    let beta_repo = repo_with_one_commit(&project, "beta");

    let mut harness = seeded_log_harness(&project, &[alpha_repo, beta_repo]);
    let alpha = root_id(harness.state(), "alpha");

    let swatches = |h: &Harness<'static, AppState>| {
        filled_circles(h)
            .into_iter()
            .filter(|(_, radius, _)| (*radius - 3.5).abs() < 0.01)
            .count()
    };
    let pane_swatches = swatches(&harness);

    open_paths_dropdown(&mut harness);
    assert_painted(&harness, ALL_REPOS);
    assert_eq!(
        swatches(&harness),
        pane_swatches + 2,
        "the Roots block paints one colour swatch per repository"
    );

    click_popup_row(&mut harness, ALL_REPOS, "beta");
    assert_eq!(
        harness.state().ui.log_roots,
        RootFilter::Only(vec![alpha.clone()]),
        "unchecking beta ticks exactly alpha — a set, not a replacement"
    );
    assert_eq!(
        popup_nodes(&harness, ALL_REPOS, "beta"),
        1,
        "the popup stayed open and the row is still there to tick back"
    );

    click_popup_row(&mut harness, ALL_REPOS, "beta");
    assert!(
        harness.state().ui.log_roots.is_all(),
        "the last tick back to a full set collapses to `All`"
    );
    assert_painted(&harness, ALL_REPOS);

    click_popup_row(&mut harness, ALL_REPOS, ALL_REPOS);
    assert!(harness.state().ui.log_roots.is_all());
    assert_painted(&harness, ALL_REPOS);
}

#[test]
fn unchecking_every_root_leaves_the_paths_button_saying_so() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    let beta = repo_with_one_commit(&project, "beta");
    let mut harness = seeded_log_harness(&project, &[alpha, beta]);

    open_paths_dropdown(&mut harness);
    click_popup_row(&mut harness, ALL_REPOS, "alpha");
    click_popup_row(&mut harness, ALL_REPOS, "beta");

    assert_eq!(
        harness.state().ui.log_roots,
        RootFilter::Only(vec![]),
        "the empty set is no repositories, not everything"
    );
    assert_painted(&harness, "Paths: No repos");
    assert_painted(&harness, "No commits matching filters");
    assert!(harness.state().ui.log_ref_scope.is_none());
}

#[test]
fn the_branch_dropdown_groups_by_kind_only_with_remotes_and_tags_collapsed() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_origin(&project, "alpha");
    git(&alpha, &["tag", "v1"]);
    let beta = repo_with_origin(&project, "beta");
    git(&beta, &["tag", "v2"]);

    let mut harness = seeded_log_harness(&project, &[alpha, beta]);
    open_branch_dropdown(&mut harness);

    for band in [LOCAL_BAND, REMOTE_BAND, TAG_BAND] {
        assert_painted(&harness, &band.to_uppercase());
        assert_eq!(
            popup_nodes(&harness, LOCAL_BAND, band),
            1,
            "{band} is one clickable header strip — of KINDS, never `alpha · {band}`"
        );
    }
    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "main"),
        1,
        "both repositories are on `main`: one deduped row for both"
    );
    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "origin/main"),
        0,
        "a folded band allocates no row at all, so its row is unclickable"
    );
    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "v1"),
        0,
        "tags start collapsed too"
    );

    let centre = harness.get_by_label(REMOTE_BAND).rect().center();
    click_at(&mut harness, centre);
    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "origin/main"),
        1,
        "a click on the header title reveals its band"
    );
    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "v1"),
        0,
        "revealing one band leaves the other folded"
    );
}

#[test]
fn a_long_ref_band_builds_only_the_rows_its_viewport_shows() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    let mut child = Command::new("git")
        .args(["update-ref", "--stdin"])
        .current_dir(&alpha)
        .stdin(Stdio::piped())
        .spawn()
        .expect("git must be on PATH");
    {
        let mut stdin = child.stdin.take().expect("a piped stdin");
        for i in 0..200 {
            writeln!(stdin, "create refs/heads/branch-{i:04} HEAD").unwrap();
        }
    }
    assert!(child.wait().expect("update-ref").success());

    let mut harness = seeded_log_harness(&project, &[alpha]);
    open_branch_dropdown(&mut harness);

    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "branch-0000"),
        1,
        "the top of the list is inside the popup's viewport"
    );
    assert_eq!(
        popup_nodes(&harness, LOCAL_BAND, "branch-0199"),
        0,
        "a row 199 rows down is outside the 320px popup and must not be built until it \
         scrolls into view"
    );

    let left = harness.get_by_label(LOCAL_BAND).rect().min.x;
    let top = harness
        .query_all_by_label("branch-0000")
        .find(|node| (node.rect().min.x - left).abs() < 2.0)
        .expect("the first row of the popup")
        .rect()
        .center();
    harness.hover_at(top);
    for _ in 0..12 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -800.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        harness.step();
    }
    settle_quiet(&mut harness);

    let bottom = harness
        .query_all_by_label("branch-0199")
        .filter(|node| (node.rect().min.x - left).abs() < 2.0)
        .count();
    assert_eq!(
        bottom, 1,
        "the same row is built once scrolling brings it into the viewport"
    );
}

/// A ref longer than the fixed menu width used to paint past the popup's own frame.
#[test]
fn a_long_branch_name_widens_the_dropdown_instead_of_overflowing_it() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    let long = "feature/a-branch-name-far-wider-than-the-old-fixed-popup";
    git(&alpha, &["branch", long]);

    let mut harness = seeded_log_harness(&project, &[alpha]);
    open_branch_dropdown(&mut harness);

    let left = harness.get_by_label(LOCAL_BAND).rect().min.x;
    let row_rect = harness
        .query_all_by_label(long)
        .find(|node| (node.rect().min.x - left).abs() < 2.0)
        .expect("the long ref is a row of the open dropdown")
        .rect();
    let painted = painted_galleys(&harness)
        .into_iter()
        .find(|galley| {
            galley.text == long
                && (galley.rect.center().y - row_rect.center().y).abs() < 2.0
                && (galley.rect.min.x - row_rect.min.x - 8.0).abs() < 2.0
        })
        .expect("the long ref's label is painted on its row");

    assert!(
        painted.rect.right() <= row_rect.right(),
        "the dropdown must widen to fit its longest ref: the label ends at {:.1}, \
         its row at {:.1}",
        painted.rect.right(),
        row_rect.right()
    );
}

/// A band that starts folded paints no row, so its longest ref must not widen the menu.
/// The reveal is what brings that ref into the layout and widens it.
#[test]
fn a_folded_band_does_not_widen_the_dropdown_until_it_is_revealed() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    let long = "release/a-tag-name-far-wider-than-the-filter-popup";
    git(&alpha, &["tag", long]);

    let mut harness = seeded_log_harness(&project, &[alpha]);
    open_branch_dropdown(&mut harness);

    let left = harness.get_by_label(LOCAL_BAND).rect().min.x;
    let main_rect = harness
        .query_all_by_label("main")
        .find(|node| (node.rect().min.x - left).abs() < 2.0)
        .expect("the open local band paints main")
        .rect();
    assert!(
        main_rect.width() <= 240.0,
        "a folded tag paints no row, so it must not widen the menu: the visible row \
         spans {:.1}",
        main_rect.width()
    );

    reveal_band(&mut harness, TAG_BAND);

    let tag_rect = harness
        .query_all_by_label(long)
        .find(|node| (node.rect().min.x - left).abs() < 2.0)
        .expect("the revealed tag is a row of the dropdown")
        .rect();
    let painted = painted_galleys(&harness)
        .into_iter()
        .find(|galley| {
            galley.text == long
                && (galley.rect.center().y - tag_rect.center().y).abs() < 2.0
                && (galley.rect.min.x - tag_rect.min.x - 8.0).abs() < 2.0
        })
        .expect("the revealed tag's label is painted on its row");

    assert!(
        painted.rect.right() <= tag_rect.right(),
        "revealing the tag must widen the menu to fit it: the label ends at {:.1}, \
         its row at {:.1}",
        painted.rect.right(),
        tag_rect.right()
    );

    reveal_band(&mut harness, TAG_BAND);
    let refolded_rect = harness
        .query_all_by_label("main")
        .find(|node| (node.rect().min.x - left).abs() < 2.0)
        .expect("the local band is still open")
        .rect();
    assert!(
        refolded_rect.width() <= 240.0,
        "folding the tag again must shrink the menu back to what it paints: the \
         visible row spans {:.1}",
        refolded_rect.width()
    );
}

#[test]
fn a_pick_switches_every_selected_repository_that_carries_the_rev() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_topic_work(&project, "alpha", "alpha topic head");
    let beta = repo_with_topic_work(&project, "beta", "beta topic head");

    let mut harness = seeded_log_harness(&project, &[alpha, beta]);
    let alpha_id = root_id(harness.state(), "alpha");
    let beta_id = root_id(harness.state(), "beta");
    harness.state_mut().selected_root = Some(alpha_id.clone());
    settle_quiet(&mut harness);

    open_branch_dropdown(&mut harness);
    click_popup_row(&mut harness, LOCAL_BAND, "topic");

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some("topic".to_owned()),
        "the name is the scope"
    );
    assert_eq!(
        harness.state().selected_root,
        Some(alpha_id.clone()),
        "the Branch dropdown never writes the selection"
    );
    assert_scope_read_settled(&harness, &alpha_id, "topic");
    assert_scope_read_settled(&harness, &beta_id, "topic");
    assert_painted(&harness, "alpha topic head");
    assert_painted(&harness, "beta topic head");
    assert_painted(&harness, "Branch: topic");
}

#[test]
fn a_selected_repository_without_the_rev_is_never_asked_for_it() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_topic_work(&project, "alpha", "alpha topic head");
    let beta = repo_with_one_commit(&project, "beta");

    let mut harness = seeded_log_harness(&project, &[alpha, beta]);
    let beta_id = root_id(harness.state(), "beta");

    open_branch_dropdown(&mut harness);
    click_popup_row(&mut harness, LOCAL_BAND, "topic");

    assert_eq!(harness.state().ui.log_ref_scope, Some("topic".to_owned()));
    assert!(
        !harness.state().caches.has_log_failure(&LogRead::Scope(
            beta_id.clone(),
            LogScope::Ref("topic".to_owned())
        )),
        "beta does not carry `topic`, so a failure there means beta was asked anyway"
    );
    assert!(
        harness.state().caches.ref_log(&beta_id, "topic").is_none(),
        "no window for `topic` may exist under beta's key"
    );
    assert!(
        harness.state().last_error.is_none(),
        "a scoped listing must not raise the global error banner for a repository \
         that was never asked: {:?}",
        harness.state().last_error
    );
    assert_painted(&harness, "alpha topic head");
}

#[test]
fn a_remote_row_filters_the_log_by_the_remote_qualified_rev() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_origin(&project, "alpha");
    git(
        &alpha,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "unfetched local work",
        ],
    );

    let mut harness = seeded_log_harness(&project, &[alpha]);
    let root = root_id(harness.state(), "alpha");

    open_dropdown_with_groups(&mut harness, &[RefGroup::Remote]);
    click_popup_row(&mut harness, LOCAL_BAND, "origin/main");

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some("origin/main".to_owned()),
        "a remote row hands git its remote-qualified rev, not its bare branch name"
    );
    assert_painted(&harness, "Branch: origin/main");

    let rows = harness
        .state()
        .caches
        .ref_log(&root, "origin/main")
        .expect("the remote scope must have been read");
    assert_eq!(
        rows.len(),
        1,
        "`origin/main` is the pushed commit only; the local branch is one ahead"
    );
    assert!(rows[0].message.contains("init"));
    assert_painted(&harness, "init");

    let painted = painted_text(&harness);
    assert!(
        !painted
            .iter()
            .any(|text| text.contains("unfetched local work")),
        "`unfetched local work` is local-only and must not appear under `origin/main`. \
         Painted:\n{painted:#?}"
    );
}

#[test]
fn a_remote_only_ref_still_resolves_after_a_pick() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_origin(&project, "alpha");
    git(&alpha, &["checkout", "-q", "-b", "solo"]);
    git(
        &alpha,
        &["commit", "-q", "--allow-empty", "-m", "solo work"],
    );
    git(&alpha, &["push", "-q", "origin", "solo"]);
    git(&alpha, &["checkout", "-q", "main"]);
    git(&alpha, &["branch", "-D", "solo"]);

    let mut harness = seeded_log_harness(&project, &[alpha]);
    let root = root_id(harness.state(), "alpha");

    open_dropdown_with_groups(&mut harness, &[RefGroup::Remote]);
    click_popup_row(&mut harness, LOCAL_BAND, "origin/solo");

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some("origin/solo".to_owned()),
        "`solo` no longer exists locally, so the row's rev must be the qualified \
         spelling or nothing answers the read"
    );
    assert_painted(&harness, "Branch: origin/solo");

    assert_scope_read_settled(&harness, &root, "origin/solo");
    let rows = harness
        .state()
        .caches
        .ref_log(&root, "origin/solo")
        .expect("the scoped read answered");
    assert!(
        rows.iter()
            .any(|commit| commit.message.contains("solo work")),
        "the log a remote-only row asks for is `solo`'s history"
    );
}

#[test]
fn picking_a_tag_filters_the_log_to_the_tag() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    git(&alpha, &["tag", "v1"]);

    let mut harness = seeded_log_harness(&project, &[alpha]);
    let root = root_id(harness.state(), "alpha");

    open_dropdown_with_groups(&mut harness, &[RefGroup::Tag]);
    click_popup_row(&mut harness, LOCAL_BAND, "v1");

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some("v1".to_owned()),
        "a tag row's rev is the tag name itself"
    );
    assert_painted(&harness, "Branch: v1");

    assert_scope_read_settled(&harness, &root, "v1");
    assert_painted(&harness, "init");
}

/// The rows a reveal brings in land below where the folded popup ended, and an `Area` keeps
/// the size it was first laid out with, so the click used to be read as a click outside and
/// shut the dropdown with no scope written at all.
#[test]
fn a_group_revealed_after_the_popup_opened_is_pickable() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_origin(&project, "alpha");
    for i in 0..5 {
        let name = format!("solo{i}");
        git(&alpha, &["branch", &name]);
        git(&alpha, &["push", "-q", "origin", &name]);
        git(&alpha, &["branch", "-D", &name]);
    }

    let mut harness = seeded_log_harness(&project, &[alpha]);
    let root = root_id(harness.state(), "alpha");
    open_branch_dropdown(&mut harness);
    reveal_band(&mut harness, REMOTE_BAND);

    let revealed = popup_nodes(&harness, REMOTE_BAND, "origin/solo0");
    assert_eq!(
        revealed, 1,
        "the reveal laid the remote rows into the popup"
    );

    click_popup_row(&mut harness, REMOTE_BAND, "origin/solo0");

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some("origin/solo0".to_owned()),
        "the click picked the row it landed on; a click read as outside this popup \
         closes the dropdown and leaves no scope at all"
    );
    assert_scope_read_settled(&harness, &root, "origin/solo0");
}

#[test]
fn the_root_gutter_shows_for_two_of_four_repositories() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let mut roots = Vec::new();
    for (name, subject) in [
        ("alpha", "alpha one"),
        ("beta", "beta two"),
        ("gamma", "gamma three"),
        ("delta", "delta four"),
    ] {
        let work = repo_with_one_commit(&project, name);
        git(&work, &["commit", "-q", "--allow-empty", "-m", subject]);
        roots.push(work);
    }
    let mut harness = seeded_log_harness(&project, &roots);

    open_paths_dropdown(&mut harness);
    click_popup_row(&mut harness, ALL_REPOS, "gamma");
    click_popup_row(&mut harness, ALL_REPOS, "delta");
    harness.get_by_label("Paths").click();
    settle_quiet(&mut harness);

    assert_eq!(
        harness
            .state()
            .ui
            .log_roots
            .selected(&harness.state().multi.roots)
            .len(),
        2,
        "two of four ticked"
    );
    let mut stripe_colors: Vec<egui::Color32> = filled_rects(&harness)
        .into_iter()
        .filter(|(r, _)| r.width() <= 5.0 && r.height() >= 15.0)
        .map(|(_, c)| c)
        .collect();
    stripe_colors.sort_by_key(|c| c.r() as u32 * 1_000_000 + c.g() as u32 * 1_000 + c.b() as u32);
    stripe_colors.dedup();
    assert!(
        stripe_colors.len() >= 2,
        "a two-member read set is exactly where the stripes must distinguish the \
         histories, got {stripe_colors:?}"
    );
    assert_painted(&harness, "Paths: 2 of 4 repos");
    assert_not_painted(&harness, "gamma three");
    assert_not_painted(&harness, "delta four");
}

fn assert_not_painted(harness: &Harness<'static, AppState>, needle: &str) {
    let painted = painted_text(harness);
    assert!(
        !painted.iter().any(|text| text.contains(needle)),
        "{needle:?} must not be painted, saw:\n{painted:#?}"
    );
}

#[test]
fn the_pane_root_rows_tick_without_overwriting_the_selection() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    let beta = repo_with_one_commit(&project, "beta");
    let gamma = repo_with_one_commit(&project, "gamma");
    let mut harness = seeded_log_harness(&project, &[alpha, beta, gamma]);
    let beta_id = root_id(harness.state(), "beta");

    open_paths_dropdown(&mut harness);
    click_popup_row(&mut harness, ALL_REPOS, "gamma");
    harness.get_by_label("Paths").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness
            .state()
            .ui
            .log_roots
            .selected(&harness.state().multi.roots)
            .len(),
        2,
        "alpha and beta ticked by unchecking gamma"
    );

    harness.get_by_label("Root alpha").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.log_roots,
        RootFilter::Only(vec![beta_id]),
        "a pane row toggles ONE member; it does not overwrite the set with itself"
    );
}

#[test]
fn the_new_branch_base_preview_reads_the_selected_repository() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_topic_work(&project, "alpha", "topic head work");
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let root = state.multi.roots[0].id.clone();
    state.selected_root = Some(root.clone());
    state.ui.log_ref_scope = Some("topic".to_owned());
    state.ensure_log(&root, LogScope::Ref("topic".to_owned()));
    state.drain_events();
    assert!(
        state.caches.log(&root).is_none(),
        "the plain window was never read here, so the base preview lives on the \
         scoped fallback"
    );
    let cid = state.caches.ref_log(&root, "topic").unwrap()[0].id.clone();
    state.ui.tab = Tab::Log;
    state.open_new_branch(Some(root), NewBranchBase::Commit(cid), false, None);
    state.ui.dialog = Some(Dialog::NewBranch);

    let mut harness = shell_harness_over(state, egui::vec2(1400.0, 900.0));
    settle_quiet(&mut harness);
    assert_painted(&harness, "topic head work");
}
