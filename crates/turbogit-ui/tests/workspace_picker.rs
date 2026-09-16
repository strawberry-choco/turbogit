//! Issue #34 follow-up — the topbar workspace picker.
//!
//! The topbar's workspace selector used to be a stub that discarded every
//! click; this suite pins the real picker end-to-end (headless egui_kittest
//! harness driving [`turbogit_ui::ui::render`], same pattern as `welcome.rs`
//! with locally-defined helpers so the file is self-contained).
//!
//! Covered here:
//! - clicking the selector opens the picker (`Open Project…` /
//!   `Attach Workspace Root…` become reachable)
//! - a `Workspace` recent row deep-scans back into the shell (both repos,
//!   including one beyond the bounded scanner's depth)
//! - a `Project` recent row takes the bounded path
//! - the current workspace is always a row, marked `current`, and clicking
//!   it is inert — the click must not re-dispatch and reset the focused root
//!   or drop caches (this is the assertion the original stub would have
//!   failed)
//! - a recents row whose directory vanished toasts instead of dispatching
//! - the picker's folder-picker entries go through the same seam as the
//!   Welcome cards
//! - Esc / click-outside dismiss the dropdown
//! - the palette's `Switch Workspace…` entry opens the same picker
//!
//! The global recents store (ADR-0005) and the directory-picker seam are
//! injected per test: a temp config dir stands in for the OS config dir and
//! closures stand in for the native folder picker.

use egui::Shape;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_app::recents::{RecentKind, RecentProject, Recents, recents_file, save};
use turbogit_app::state::{AppState, ToastKind};

// --- Locally-defined harness helpers (same pattern as welcome.rs) -------------

fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

#[track_caller]
fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

/// Step frames until the painted output stabilizes.
fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("picker layout did not settle within 10 frames");
}

/// Run `git <args>` in `cwd`, panicking on failure (tests need real repos).
#[track_caller]
fn git(args: &[&str], cwd: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Create a real repository with a deterministic initial branch (`main`).
fn seed_repo(base: &Path, name: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&["init", "-q", "-b", "main"], &dir);
    dir
}

/// A workspace container with two nested repos, one strictly deeper than the
/// bounded scanner's `SCAN_MAX_DEPTH` so only the deep scan finds it.
fn seed_workspace(base: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).expect("create workspace dir");
    let shallow = seed_repo(&ws, "alpha");
    let deep = ws.join("a").join("b").join("c").join("d");
    std::fs::create_dir_all(&deep).expect("create deep dir");
    git(&["init", "-q", "-b", "main"], &deep);
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

/// Seed the global recents file under a TEMP config dir (never the real one).
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

/// A harness over `launch` (empty → Welcome) with `picker` as the injected
/// folder picker and `recents` seeded into a temp config dir first.
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

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    Fixture {
        harness,
        _config: config,
    }
}

/// Shell-up fixture: launched straight into `launch` with no picker wired.
fn fixture_at(launch: PathBuf) -> Fixture {
    fixture(Some(launch), Some(Box::new(|| None)), &[])
}

/// Shell-up fixture whose injected folder picker returns `pick`.
fn fixture_at_with_picker(
    launch: PathBuf,
    pick: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
) -> Fixture {
    fixture(Some(launch), Some(Box::new(pick)), &[])
}

/// Shell-up fixture with seeded recents.
fn fixture_at_with_recents(launch: PathBuf, recents: &[RecentProject]) -> Fixture {
    fixture(Some(launch), Some(Box::new(|| None)), recents)
}

// --- Label helpers -------------------------------------------------------------

/// The available button labels, for failure messages.
fn button_labels(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .get_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

/// A Button whose accessible label is exactly `label`.
///
/// Scoped to `Role::Button` on purpose: the workspace selector's text also
/// paints as the breadcrumb's first crumb (a plain Label), so a bare
/// `get_by_label` would match whichever node the tree hands back first.
#[track_caller]
fn button<'t>(harness: &'t Harness<'_, AppState>, label: &'t str) -> Node<'t> {
    harness
        .query_by_role_and_label(Role::Button, label)
        .unwrap_or_else(|| {
            panic!(
                "no button labelled {label:?}; buttons: {:?}",
                button_labels(harness)
            )
        })
}

/// The first Button whose accessible label contains `needle`. Used for the
/// picker's decorated rows, whose labels are the row name plus its count /
/// `current` markers.
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

/// Open the picker by clicking the real topbar selector — the click path the
/// original stub silently dropped.
#[track_caller]
fn open_picker(fx: &mut Fixture) {
    let name = fx
        .harness
        .state()
        .project_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<workspace>".to_string());
    button(&fx.harness, &name).click();
    settle(&mut fx.harness);
}

// --- Cycle 1: the selector opens the picker ------------------------------------

#[test]
fn selector_click_opens_the_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at(repo);
    settle(&mut fx.harness);

    // The Welcome page owns these strings too, but the shell is up.
    assert_not_painted(&fx.harness, "Open Project…");
    assert_not_painted(&fx.harness, "Attach Workspace Root…");
    assert!(!fx.harness.state().ui.workspace_picker_open);

    open_picker(&mut fx);

    assert_painted(&fx.harness, "Open Project…");
    assert_painted(&fx.harness, "Attach Workspace Root…");
    assert!(
        fx.harness.state().ui.workspace_picker_open,
        "the selector click must open the picker"
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

    // The picker anchors under the selector (top-left); the topbar's Push
    // button sits at the opposite corner of that band.
    let push = fx
        .harness
        .get_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().as_deref() == Some("Push") && n.rect().top() < 38.0)
        .expect("the topbar Push button");
    push.click();
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "Open Project…");
    assert!(!fx.harness.state().ui.workspace_picker_open);
}

// --- Cycle 2: rows switch workspaces -------------------------------------------

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
    // The workspace row paints its indexed repo count.
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

// --- Cycle 3: the current workspace is a marked, inert row ----------------------

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

    // A genuinely attached workspace: 2 roots, `ws` as the project dir.
    fx.harness.state_mut().attach_workspace(&ws);
    settle(&mut fx.harness);

    // Sentinel state a re-dispatch would wipe: the focused root and one
    // cache entry.
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

    // Reopen the picker (this test is about the row, not the selector click).
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

// --- Cycle 4: the folder-picker entries share the Welcome seam ------------------

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

// --- Cycle 5: a vanished recents row --------------------------------------------

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

// --- Cycle 6: the palette opens the same picker ---------------------------------

#[test]
fn palette_entry_opens_the_same_picker() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = seed_repo(scratch.path(), "alpha");
    let mut fx = fixture_at(repo);
    settle(&mut fx.harness);

    fx.harness.state_mut().ui.command_palette = true;
    fx.harness.state_mut().ui.command_query = "switch workspace".to_string();
    settle(&mut fx.harness);

    button(&fx.harness, "Switch Workspace…").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert!(
        s.ui.workspace_picker_open,
        "the palette entry opens the topbar picker"
    );
    assert!(
        s.ui.workspace_picker_anchor.is_none(),
        "the palette route has no selector anchor"
    );
    assert!(
        !s.ui.command_palette,
        "picking a palette entry closes the palette"
    );
    assert_painted(&fx.harness, "Open Project…");
}
