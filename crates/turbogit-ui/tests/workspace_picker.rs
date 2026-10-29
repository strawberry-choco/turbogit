use egui::accesskit::Role;
use egui::epaint::color::ColorMode;
use egui::{Color32, Shape};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{assert_not_painted, assert_painted, settle, shell_harness_over};
use turbogit_app::recents::{RecentKind, RecentProject, Recents, recents_file, save};
use turbogit_app::state::{AppState, ToastKind};
use turbogit_ui::theme::{ITEM_SPACING, Palette};

fn seed_repo(base: &Path, name: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&dir, &["init", "-q", "-b", "main"]);
    dir
}

fn seed_workspace(base: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).expect("create workspace dir");
    let shallow = seed_repo(&ws, "alpha");
    let deep = ws.join("a").join("b").join("c").join("d");
    std::fs::create_dir_all(&deep).expect("create deep dir");
    git(&deep, &["init", "-q", "-b", "main"]);
    (ws, shallow, deep)
}

fn recent(path: &Path, name: &str, kind: RecentKind, repo_count: Option<usize>) -> RecentProject {
    RecentProject {
        path: path.to_path_buf(),
        name: name.to_string(),
        last_opened: 1_755_000_000_000,
        kind,
        repo_count,
    }
}

fn seed_recents(config_dir: &Path, projects: &[RecentProject]) {
    let file = recents_file(config_dir);
    std::fs::create_dir_all(file.parent().unwrap()).expect("create config dir");
    save(
        config_dir,
        &Recents {
            projects: projects.to_vec(),
        },
    )
    .expect("seed recents file");
}

struct Fixture {
    harness: Harness<'static, AppState>,
    /// Injected OS-config-dir stand-in holding the global recents file.
    _config: tempfile::TempDir,
}

fn fixture(
    launch: Option<PathBuf>,
    picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    recents: &[RecentProject],
) -> Fixture {
    let config = tempfile::tempdir().expect("temp config dir");
    if !recents.is_empty() {
        seed_recents(config.path(), recents);
    }
    let mut state = AppState::launch_in(launch, Some(config.path().to_path_buf()));
    state.dir_picker = picker;

    Fixture {
        harness: shell_harness_over(state, egui::vec2(1024.0, 768.0)),
        _config: config,
    }
}

fn fixture_at(launch: PathBuf) -> Fixture {
    fixture(Some(launch), Some(Box::new(|| None)), &[])
}

fn fixture_at_with_picker(
    launch: PathBuf,
    pick: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
) -> Fixture {
    fixture(Some(launch), Some(Box::new(pick)), &[])
}

fn fixture_at_with_recents(launch: PathBuf, recents: &[RecentProject]) -> Fixture {
    fixture(Some(launch), Some(Box::new(|| None)), recents)
}

fn fixture_at_wide(launch: PathBuf, recents: &[RecentProject]) -> Fixture {
    let mut fx = fixture(Some(launch), Some(Box::new(|| None)), recents);
    fx.harness.set_size(egui::vec2(1280.0, 768.0));
    fx
}

fn button_labels(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .get_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

/// The node whose accessible label is exactly `label`.
///
/// No role scoping: the topbar selector that used to make the project name
/// ambiguous is gone, so every label below identifies exactly one node.
/// `query_by_label` still panics when a label is ambiguous, which is the
/// signal that two surfaces have started sharing a name.
#[track_caller]
fn button<'t>(harness: &'t Harness<'_, AppState>, label: &'t str) -> Node<'t> {
    harness.query_by_label(label).unwrap_or_else(|| {
        panic!(
            "no node labelled {label:?}; buttons: {:?}",
            button_labels(harness)
        )
    })
}

#[track_caller]
fn button_containing<'t>(harness: &'t Harness<'_, AppState>, needle: &'t str) -> Node<'t> {
    harness
        .query_all_by_label_contains(needle)
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap_or_else(|| {
            panic!(
                "no button labelled with {needle:?}; buttons: {:?}",
                button_labels(harness)
            )
        })
}

#[track_caller]
fn open_picker(fx: &mut Fixture) {
    fx.harness.state_mut().ui.command_palette = true;
    fx.harness.state_mut().ui.command_query = "switch workspace".to_string();
    settle(&mut fx.harness);
    button(&fx.harness, "Switch Workspace…").click();
    settle(&mut fx.harness);
    assert!(
        fx.harness.state().ui.workspace_picker_open,
        "the palette entry must open the picker"
    );
}

#[test]
fn escape_closes_the_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at(repo);
    settle(&mut fx.harness);

    open_picker(&mut fx);
    assert_painted(&fx.harness, "Open Project…");

    fx.harness.key_press(egui::Key::Escape);
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "Open Project…");
    assert!(!fx.harness.state().ui.workspace_picker_open);
}

#[test]
fn clicking_outside_the_picker_closes_it() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at(repo);
    settle(&mut fx.harness);

    open_picker(&mut fx);
    assert_painted(&fx.harness, "Open Project…");

    button(&fx.harness, "Refresh changes").click();
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "Open Project…");
    assert!(!fx.harness.state().ui.workspace_picker_open);
}

#[test]
fn picker_row_switches_to_a_recent_workspace() {
    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let (ws, shallow, deep) = seed_workspace(scratch.path());
    let mut fx = fixture_at_with_recents(
        current,
        &[recent(&ws, "ws", RecentKind::Workspace, Some(2))],
    );
    settle(&mut fx.harness);

    open_picker(&mut fx);
    assert_painted(&fx.harness, "ws");
    assert_painted(&fx.harness, "2 repos");

    button_containing(&fx.harness, "ws").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert_eq!(
        s.project_dir, ws,
        "the workspace row retargets the project dir"
    );
    assert!(!s.show_welcome(), "switching enters the shell");
    assert_eq!(
        s.multi.roots.len(),
        2,
        "a Workspace row deep-scans: both repos register"
    );
    assert!(
        s.multi.roots.iter().any(|r| r.id.as_path() == shallow),
        "the shallow repo registers"
    );
    assert!(
        s.multi.roots.iter().any(|r| r.id.as_path() == deep),
        "the beyond-SCAN_MAX_DEPTH repo registers"
    );
    assert!(!s.ui.workspace_picker_open, "switching closes the picker");
}

#[test]
fn picker_row_switches_to_a_recent_project() {
    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let beta = seed_repo(scratch.path(), "beta");
    let mut fx =
        fixture_at_with_recents(current, &[recent(&beta, "beta", RecentKind::Project, None)]);
    settle(&mut fx.harness);

    open_picker(&mut fx);
    button(&fx.harness, "beta").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert_eq!(s.project_dir, beta);
    assert!(!s.show_welcome(), "switching enters the shell");
    assert_eq!(
        s.multi.roots.len(),
        1,
        "a Project row takes the bounded scan"
    );
    assert_eq!(s.multi.roots[0].id.as_path(), beta.as_path());
    assert!(!s.ui.workspace_picker_open);
}

#[test]
fn current_workspace_row_is_marked_and_inert() {
    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let (ws, _, _) = seed_workspace(scratch.path());
    let mut fx = fixture_at_with_recents(
        current,
        &[recent(&ws, "ws", RecentKind::Workspace, Some(2))],
    );
    settle(&mut fx.harness);

    fx.harness.state_mut().attach_workspace(&ws);
    settle(&mut fx.harness);

    let root_id = fx
        .harness
        .state()
        .selected_root
        .clone()
        .expect("a focused root after attaching");
    fx.harness
        .state_mut()
        .caches
        .store_ahead_behind(root_id.clone(), (7, 9));

    fx.harness.state_mut().ui.workspace_picker_open = true;
    settle(&mut fx.harness);

    let before_dir = fx.harness.state().project_dir.clone();
    let before_selected = fx.harness.state().selected_root.clone();
    let before_cache = fx
        .harness
        .state()
        .caches
        .ahead_behind(&root_id)
        .expect("the sentinel cache entry exists");

    button_containing(&fx.harness, "current").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert_eq!(
        s.project_dir, before_dir,
        "clicking the current row must not re-dispatch"
    );
    assert_eq!(
        s.selected_root, before_selected,
        "clicking the current row must not reset the focused root"
    );
    assert_eq!(
        s.caches.ahead_behind(&root_id),
        Some(before_cache),
        "clicking the current row must not drop caches"
    );
    assert!(
        !s.ui.workspace_picker_open,
        "the click still closes the picker"
    );
}

#[test]
fn open_project_entry_goes_through_the_picker_seam() {
    static PICKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    use std::sync::atomic::Ordering::SeqCst;

    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let picked = seed_repo(scratch.path(), "picked");
    let p = picked.clone();
    let mut fx = fixture_at_with_picker(current, move || {
        PICKS.fetch_add(1, SeqCst);
        Some(p.clone())
    });
    settle(&mut fx.harness);
    PICKS.store(0, SeqCst);

    open_picker(&mut fx);
    button(&fx.harness, "Open Project…").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert_eq!(
        s.project_dir, picked,
        "Open Project… enters the picked repository"
    );
    assert!(!s.show_welcome());
    assert_eq!(
        PICKS.load(SeqCst),
        1,
        "one click goes through the picker seam exactly once"
    );
}

#[test]
fn attach_entry_indexes_the_whole_tree() {
    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let (ws, _, deep) = seed_workspace(scratch.path());
    let w = ws.clone();
    let mut fx = fixture_at_with_picker(current, move || Some(w.clone()));
    settle(&mut fx.harness);

    open_picker(&mut fx);
    button(&fx.harness, "Attach Workspace Root…").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert_eq!(
        s.project_dir, ws,
        "the workspace root becomes the project dir"
    );
    assert!(!s.show_welcome());
    assert_eq!(
        s.multi.roots.len(),
        2,
        "the deep scan indexes repos beyond SCAN_MAX_DEPTH"
    );
    assert!(
        s.multi.roots.iter().any(|r| r.id.as_path() == deep),
        "the beyond-SCAN_MAX_DEPTH repo registers"
    );
}

#[test]
fn missing_recent_path_toasts_instead_of_dispatching() {
    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let gone = seed_repo(scratch.path(), "gone");
    let mut fx = fixture_at_with_recents(
        current.clone(),
        &[recent(&gone, "gone", RecentKind::Project, None)],
    );
    settle(&mut fx.harness);
    std::fs::remove_dir_all(&gone).expect("delete the recent's directory");

    open_picker(&mut fx);
    button(&fx.harness, "gone").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert_eq!(
        s.project_dir, current,
        "a missing path must not retarget the project dir"
    );
    assert!(
        !s.show_welcome(),
        "a missing path must not bounce the user to Welcome"
    );
    let toast =
        s.ui.toast
            .as_ref()
            .expect("a missing recents path surfaces an error toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(
        toast.message.contains("gone"),
        "the toast names the missing path; got {:?}",
        toast.message
    );
    assert_painted(&fx.harness, "no longer exists");
}

#[test]
fn palette_entry_opens_the_same_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at(repo);
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "Open Project…");
    assert!(!fx.harness.state().ui.workspace_picker_open);

    fx.harness.state_mut().ui.command_palette = true;
    fx.harness.state_mut().ui.command_query = "switch workspace".to_string();
    settle(&mut fx.harness);

    button(&fx.harness, "Switch Workspace…").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert!(
        s.ui.workspace_picker_open,
        "the palette entry opens the picker"
    );
    assert!(
        s.ui.workspace_picker_anchor.is_none(),
        "the palette route has no trigger anchor"
    );
    assert!(
        !s.ui.command_palette,
        "picking a palette entry closes the palette"
    );
    assert_painted(&fx.harness, "Open Project…");
}

#[track_caller]
fn header<'t>(harness: &'t Harness<'_, AppState>) -> Node<'t> {
    button(harness, "Switch workspace")
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

/// The inks stroked by the row's glyphs inside `band`, the brand folder icon
/// and the chevron. Text paints as galleys, so this is the icon vocabulary.
fn glyph_inks(harness: &Harness<'_, AppState>, band: egui::Rect) -> Vec<Color32> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Path(path)
                if !path.points.is_empty()
                    && path.points.iter().all(|p| band.expand(2.0).contains(*p)) =>
            {
                match path.stroke.color {
                    ColorMode::Solid(color) => Some(color),
                    ColorMode::UV(_) => None,
                }
            }
            _ => None,
        })
        .collect()
}

#[test]
fn clicking_the_sidebar_header_opens_the_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let current = seed_repo(scratch.path(), "current");
    let (ws, _, _) = seed_workspace(scratch.path());
    let mut fx = fixture_at_wide(
        current,
        &[recent(&ws, "ws", RecentKind::Workspace, Some(2))],
    );
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "Open Project…");
    assert!(!fx.harness.state().ui.workspace_picker_open);

    let band = header(&fx.harness).rect();
    assert_eq!(
        band.width(),
        turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        "the header's hit target spans the full rail, insets included"
    );

    let aim = egui::Pos2::new(band.right() - 70.0, band.center().y);

    click_at(&mut fx.harness, aim);

    let s = fx.harness.state();
    assert!(
        s.ui.workspace_picker_open,
        "the header click opens the picker"
    );
    let (x, y) =
        s.ui.workspace_picker_anchor
            .expect("the header click anchors the dropdown to the row");
    assert!(
        (x - band.left()).abs() < 0.5 && (y - (band.bottom() + ITEM_SPACING.y)).abs() < 0.5,
        "the anchor hangs the dropdown from the row's bottom-left; got ({x}, {y}), want ({}, {})",
        band.left(),
        band.bottom() + ITEM_SPACING.y
    );

    assert_painted(&fx.harness, "ws");
    assert_painted(&fx.harness, "2 repos");
    assert_painted(&fx.harness, "current");
    assert_painted(&fx.harness, "Open Project…");
    assert_painted(&fx.harness, "Attach Workspace Root…");
}

#[test]
fn hovering_the_sidebar_header_offers_the_switch() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at_wide(repo, &[]);
    settle(&mut fx.harness);

    let band = header(&fx.harness).rect();
    let aim = egui::Pos2::new(band.right() - 70.0, band.center().y);
    let resting = glyph_inks(&fx.harness, band);
    assert!(
        resting.contains(&Palette::INK_3) && resting.contains(&Palette::BRAND),
        "the row paints the muted chevron and the brand folder; got {resting:?}"
    );

    fx.harness.hover_at(aim);
    // The tooltip waits for a still pointer, so hold the hover for a beat.
    for _ in 0..6 {
        fx.harness.step();
    }

    let hovering = glyph_inks(&fx.harness, band);
    assert!(
        hovering.contains(&Palette::INK_2) && !hovering.contains(&Palette::INK_3),
        "the chevron steps one up the ink ramp on hover; got {hovering:?}"
    );
    assert_eq!(
        fx.harness.output().platform_output.cursor_icon,
        egui::CursorIcon::PointingHand,
        "the row asks for the hand, so it reads as clickable"
    );
    assert_painted(&fx.harness, "Switch workspace");
    assert!(
        !fx.harness.state().ui.workspace_picker_open,
        "a hover is not a click"
    );
}

#[test]
fn clicking_the_header_again_closes_the_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at_wide(repo, &[]);
    settle(&mut fx.harness);

    let band = header(&fx.harness).rect();
    let aim = egui::Pos2::new(band.left() + 150.0, band.center().y);
    click_at(&mut fx.harness, aim);
    assert!(
        fx.harness.state().ui.workspace_picker_open,
        "the header opens the picker"
    );
    assert_painted(&fx.harness, "Open Project…");

    click_at(&mut fx.harness, aim);

    assert!(
        !fx.harness.state().ui.workspace_picker_open,
        "a second click on the trigger dismisses the dropdown, as click-outside does"
    );
    assert_not_painted(&fx.harness, "Open Project…");
}
