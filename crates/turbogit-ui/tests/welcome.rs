//! Issue #10 — Welcome screen core: action cards, global recents, launch flow.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] end-to-end
//! (same pattern as `shell_frame.rs`, with locally-defined helpers so
//! this file is self-contained). Asserts only on public surfaces:
//!
//! - **Painted output** — text galleys from the frame's shapes.
//!
//! The global recents store (ADR-0005) and the directory-picker seam are
//! injected per test: a temp config dir stands in for the OS config dir and
//! closures stand in for the native folder picker, so no test ever touches
//! the real user configuration or shows a modal dialog.
//!
//! Covered here (spec §8.1, ADR-0004, ADR-0005):
//! - brand header + three action cards + inline clone box paint on Welcome
//! - Open card opens a real repository into the shell (end-to-end)
//! - Initialize card creates a repository and enters it (end-to-end)
//! - seeded recents render name / path / last-opened + live branch indicator
//! - clicking a recent reopens that project
//! - branch indicators are cached in memory, then recompute when invalidated
//! - File → Welcome closes every project and returns to the screen
//! - `turbogit <path>` bypasses Welcome; launching without one lands on it
//!
//! Issue #17 adds the Clone card end-to-end and pins the folder-picker seam:
//! - Clone from a URL (plain local path) into a picked destination with full
//!   history; the clone enters the shell and is offered in recents
//! - The shallow checkbox limits cloned history to `--depth 1` (via a
//!   `file://` remote — the only local transport that honors depth)
//! - Missing picker / cancelled pick surface toasts instead of failing
//! - The picker seam is invoked only behind user-initiated flows

use egui::Shape;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_app::recents::{RecentProject, Recents, load, recents_file, record, save};
use turbogit_app::state::AppState;

// --- Locally-defined harness helpers -----------------------------------------

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
    panic!("welcome layout did not settle within 10 frames");
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
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Create a real repository with a deterministic initial branch (`main`).
fn seed_repo(base: &Path, name: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&["init", "-b", "main"], &dir);
    dir
}

/// Run `git <args>` in `cwd`, returning trimmed stdout (tests read history).
#[track_caller]
fn git_out(args: &[&str], cwd: &Path) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A real bare "remote" with three commits on `main` (issue #17).
fn seed_bare_remote(base: &Path, name: &str) -> PathBuf {
    let work = seed_repo(base, &format!("{name}-work"));
    git(&["config", "user.email", "test@example.com"], &work);
    git(&["config", "user.name", "Test"], &work);
    for i in 1..=3 {
        std::fs::write(work.join(format!("f{i}.txt")), format!("commit {i}"))
            .expect("write tracked file");
        git(&["add", "."], &work);
        git(&["commit", "-q", "-m", &format!("c{i}")], &work);
    }
    let bare = base.join(format!("{name}.git"));
    let work_s = work.to_string_lossy().to_string();
    let bare_s = bare.to_string_lossy().to_string();
    git(&["clone", "--bare", "-q", &work_s, &bare_s], base);
    bare
}

/// `file:///…` URL for a local path: the only local transport that honors
/// `--depth` (plain paths make git ignore it with a warning).
fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

/// Focus the input labelled `label` and type into it (kittest only delivers
/// text events to the focused widget, so click-to-focus must come first).
#[track_caller]
fn type_into(harness: &mut Harness<'_, AppState>, label: &str, text: &str) {
    let field = harness.get_by_label(label);
    field.click();
    field.type_text(text);
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
    /// The launch project dir (empty → Welcome).
    _project: tempfile::TempDir,
    /// Injected OS-config-dir stand-in holding the global recents file.
    config: tempfile::TempDir,
}

/// A harness over an empty project dir (Welcome visible) with an injected,
/// empty recents config dir. `pick` becomes the injected folder picker.
fn fixture_with_picker(pick: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) -> Fixture {
    fixture_inner(Some(Box::new(pick)))
}

/// Same, but with NO folder picker wired at all (production always injects
/// `rfd`; this exercises the seam's missing-picker path).
fn fixture_without_picker() -> Fixture {
    fixture_inner(None)
}

fn fixture_inner(picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>) -> Fixture {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let mut state = AppState::launch_in(None, Some(config.path().to_path_buf()));
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
        _project: project,
        config,
    }
}

fn bare_fixture() -> Fixture {
    fixture_with_picker(|| None)
}

// --- Cycle 1: the page paints -------------------------------------------------

#[test]
fn welcome_paints_brand_and_three_action_cards() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "TurboGit");
    assert_painted(&fx.harness, "A fast, keyboard-friendly Git client");
    // Three action cards (spec §8.1).
    assert_painted(&fx.harness, "Clone from URL");
    assert_painted(&fx.harness, "Open Project");
    assert_painted(&fx.harness, "Initialize Repository");
    // Recents column exists even when empty.
    assert_painted(&fx.harness, "RECENT PROJECTS");
    assert_painted(&fx.harness, "No recent projects yet.");
}

#[test]
fn clone_box_offers_url_input_and_clone_action() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "Repository URL");
    assert_painted(&fx.harness, "Clone");
    assert_painted(&fx.harness, "Shallow clone");
    // Getting-started hints close the page (spec §8.1 item 3); group titles
    // render uppercase.
    assert_painted(&fx.harness, "GETTING STARTED");
}

// --- Cycle 2: Open / Initialize cards are end-to-end --------------------------

#[test]
fn open_card_opens_a_real_repository_into_the_shell() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = seed_repo(project.path(), "alpha");

    let picked = repo.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);

    fx.harness.get_by_label("Open Project").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert!(
        !s.show_welcome(),
        "opening a real repository must enter the shell"
    );
    assert_eq!(s.multi.roots.len(), 1, "the opened repo is registered");
    assert_eq!(s.multi.roots[0].id.as_path(), repo.as_path());
    assert_eq!(
        s.selected_root.as_ref().map(|r| r.0.to_path_buf()),
        Some(repo)
    );
    // The welcome page is gone; the shell status bar reports the root.
    assert_not_painted(&fx.harness, "A fast, keyboard-friendly Git client");
    assert_painted(&fx.harness, "modified:");
}

#[test]
fn initialize_card_creates_a_repo_and_enters_it() {
    let project = tempfile::tempdir().expect("temp project dir");
    let target = project.path().join("fresh-init");
    std::fs::create_dir_all(&target).expect("create init target");

    let picked = target.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);

    fx.harness.get_by_label("Initialize Repository").click();
    settle(&mut fx.harness);

    assert!(
        target.join(".git").exists(),
        "the Initialize card must create a real repository"
    );
    let s = fx.harness.state();
    assert!(!s.show_welcome(), "initializing must enter the shell");
    assert_eq!(s.multi.roots.len(), 1);
    assert_eq!(s.multi.roots[0].id.as_path(), target.as_path());
}

// --- Cycle 3: seeded recents render and reopen --------------------------------

#[test]
fn seeded_recents_render_name_path_last_opened_and_live_branch() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_000,
        }],
    );

    let cfg = config.path().to_path_buf();
    let state = AppState::launch_in(None, Some(cfg));
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
    settle(&mut harness);

    assert_painted(&harness, "alpha");
    // The project location is painted on the row. Long paths are
    // middle-truncated for the narrow column, so assert a path-like galley
    // that names the repo folder rather than the full absolute string.
    let sep = std::path::MAIN_SEPARATOR;
    let texts = painted_text(&harness);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("alpha") && t != "alpha" && t.contains(sep)),
        "recent row must paint the project path; painted text:\n{texts:#?}"
    );
    assert_painted(&harness, "Last opened");
    // Branch indicator computed live at render time (ADR-0005): the repo's
    // current branch is painted next to the recent row.
    assert_painted(&harness, "main");
}

#[test]
fn clicking_a_recent_reopens_the_project() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_000,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| turbogit_ui::theme::install_fonts(ui.ctx()));
            turbogit_ui::ui::render(ui, state);
        },
        AppState::launch_in(None, Some(cfg)),
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    settle(&mut harness);

    harness.get_by_label("alpha").click();
    settle(&mut harness);

    let s = harness.state();
    assert!(!s.show_welcome(), "clicking a recent must enter the shell");
    assert_eq!(
        s.selected_root.as_ref().map(|r| r.0.to_path_buf()),
        Some(repo)
    );
    assert!(!s.ui.welcome_visible);
}

// --- Cycle 4: branch indicators are live-at-render with in-memory caching -----

#[test]
fn branch_indicator_is_cached_then_updates_after_invalidation() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_000,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| turbogit_ui::theme::install_fonts(ui.ctx()));
            turbogit_ui::ui::render(ui, state);
        },
        AppState::launch_in(None, Some(cfg)),
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    settle(&mut harness);
    assert_painted(&harness, "main");

    // Mutate the repo OUTSIDE TurboGit: switch to another branch.
    git(&["checkout", "-b", "feature/next"], &repo);
    settle(&mut harness);
    assert_painted(&harness, "main"); /* still cached — indicators are never re-shelled every frame */
    assert_not_painted(&harness, "feature/next");

    // Invalidate the in-memory cache: the very next render recomputes LIVE.
    harness.state_mut().invalidate_welcome_branches();
    settle(&mut harness);
    assert_painted(&harness, "feature/next");
    assert_not_painted(&harness, "main");
}

// --- Cycle 5: File → Welcome and the CLI launch flow (ADR-0004) ---------------

#[test]
fn file_menu_welcome_closes_projects_and_returns_to_welcome() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = seed_repo(project.path(), "alpha");

    let mut harness = Harness::new_ui_state(
        |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| turbogit_ui::theme::install_fonts(ui.ctx()));
            turbogit_ui::ui::render(ui, state);
        },
        AppState::launch(Some(repo.clone())),
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    settle(&mut harness);
    assert!(
        !harness.state().show_welcome(),
        "launching with a path must enter the shell directly"
    );

    harness.get_by_label("File").click();
    settle(&mut harness);
    harness.get_by_label("Welcome Screen").click();
    settle(&mut harness);

    let s = harness.state();
    assert!(s.show_welcome(), "File → Welcome must return to the screen");
    assert!(
        s.multi.roots.is_empty(),
        "File → Welcome must close every open project"
    );
    assert!(s.ui.welcome_visible);
    assert_painted(&harness, "A fast, keyboard-friendly Git client");
}

#[test]
fn cli_path_argument_bypasses_welcome_but_no_argument_lands_on_it() {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = seed_repo(project.path(), "alpha");

    // `turbogit <path>`: straight into the shell.
    let state = AppState::launch(Some(repo));
    assert!(!state.show_welcome());
    assert_eq!(state.multi.roots.len(), 1);

    // Bare launch: Welcome, regardless of what the process CWD happens to be.
    let config = tempfile::tempdir().expect("temp config dir");
    let state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    assert!(state.show_welcome());
    assert!(state.multi.roots.is_empty());
}

// --- Cycle 6: the global recents store itself (ADR-0005) -----------------------

#[test]
fn recents_store_roundtrips_upserts_sorts_and_caps() {
    let config = tempfile::tempdir().expect("temp config dir");

    // Missing file loads as empty.
    assert!(load(config.path()).projects.is_empty());

    // Recording derives the name from the final path component.
    let a = config.path().join("repos").join("alpha");
    let b = config.path().join("repos").join("beta");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    record(config.path(), &a);
    std::thread::sleep(std::time::Duration::from_millis(20));
    record(config.path(), &b);

    let recents = load(config.path());
    assert_eq!(recents.projects.len(), 2);
    assert_eq!(recents.projects[0].name, "beta", "newest first");
    assert_eq!(recents.projects[1].name, "alpha");
    assert!(
        recents.projects[0].last_opened >= recents.projects[1].last_opened,
        "sorted by last_opened descending"
    );

    // Re-recording upserts in place (no duplicate rows).
    record(config.path(), &a);
    let recents = load(config.path());
    assert_eq!(recents.projects.len(), 2, "upsert must not duplicate");
    assert_eq!(recents.projects[0].name, "alpha", "re-opened moves to top");

    // The store caps at MAX_RECENTS entries.
    for i in 0..(turbogit_app::recents::MAX_RECENTS + 4) {
        let p = config.path().join(format!("r{i}"));
        std::fs::create_dir_all(&p).unwrap();
        record(config.path(), &p);
    }
    let recents = load(config.path());
    assert_eq!(
        recents.projects.len(),
        turbogit_app::recents::MAX_RECENTS,
        "store must cap at MAX_RECENTS"
    );

    // A corrupt file degrades to empty instead of crashing the app.
    let file = recents_file(config.path());
    std::fs::write(&file, "not ron at all {{{").unwrap();
    assert!(load(config.path()).projects.is_empty());
}

// --- Cycle 7: Clone card is end-to-end (issue #17) ------------------------------

/// The clone must be offered by the global recents store: both the in-memory
/// copy on `AppState` and the persisted file under the injected config dir.
#[track_caller]
fn assert_offered_in_recents(fx: &Fixture, dest: &Path) {
    let s = fx.harness.state();
    assert!(
        s.ui.recent_projects.iter().any(|p| p.path == dest),
        "cloned repo must appear in in-memory recents; got {:?}",
        s.ui.recent_projects
    );
    let persisted = load(fx.config.path());
    assert!(
        persisted.projects.iter().any(|p| p.path == dest),
        "cloned repo must be recorded in the persisted recents store"
    );
}

#[test]
fn clone_card_clones_full_history_from_a_local_path_into_the_picked_destination() {
    let project = tempfile::tempdir().expect("temp project dir");
    let remote = seed_bare_remote(project.path(), "origin");
    let workspace = project.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create picked parent");
    let dest = workspace.join("origin");

    let picked = workspace.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);

    type_into(&mut fx.harness, "Repository URL", remote.to_str().unwrap());
    fx.harness.get_by_label("Clone").click();
    settle(&mut fx.harness);

    // Real files on disk: a full clone of the bare remote.
    assert!(
        dest.join(".git").exists(),
        "clone must land at <picked parent>/origin"
    );
    assert_eq!(
        git_out(&["rev-list", "--count", "HEAD"], &dest),
        "3",
        "a full clone carries the remote's entire history"
    );

    // It opens into the shell like any other project.
    let s = fx.harness.state();
    assert!(!s.show_welcome(), "a successful clone enters the shell");
    assert_eq!(s.multi.roots.len(), 1);
    assert_eq!(s.multi.roots[0].id.as_path(), dest.as_path());
    assert_eq!(
        s.selected_root.as_ref().map(|r| r.0.to_path_buf()),
        Some(dest.clone())
    );
    assert!(
        s.ui.welcome_clone_url.is_empty(),
        "URL input resets on success"
    );

    assert_painted(&fx.harness, "Repository cloned");
    assert_offered_in_recents(&fx, &dest);
}

#[test]
fn clone_card_shallow_checkbox_limits_cloned_history_to_depth_one() {
    let project = tempfile::tempdir().expect("temp project dir");
    let remote = seed_bare_remote(project.path(), "origin");
    let workspace = project.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create picked parent");
    let dest = workspace.join("origin");

    let picked = workspace.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);

    type_into(&mut fx.harness, "Repository URL", &file_url(&remote));
    fx.harness.get_by_label("Shallow clone (--depth 1)").click();
    settle(&mut fx.harness);
    assert!(
        fx.harness.state().ui.welcome_shallow,
        "clicking the checkbox must toggle shallow mode on"
    );
    fx.harness.get_by_label("Clone").click();
    settle(&mut fx.harness);

    assert!(
        dest.join(".git").exists(),
        "shallow clone still lands on disk"
    );
    assert_eq!(
        git_out(&["rev-list", "--count", "HEAD"], &remote),
        "3",
        "the remote keeps its full history"
    );
    assert_eq!(
        git_out(&["rev-list", "--count", "HEAD"], &dest),
        "1",
        "shallow clone must pass --depth 1 through the engine layer"
    );

    let s = fx.harness.state();
    assert!(!s.show_welcome());
    assert_eq!(s.multi.roots[0].id.as_path(), dest.as_path());
    assert_offered_in_recents(&fx, &dest);
}

// --- Cycle 8: the folder-picker seam (issue #17) ---------------------------------

#[test]
fn clone_without_a_folder_picker_surfaces_a_toast_and_stays_on_welcome() {
    let mut fx = fixture_without_picker();
    settle(&mut fx.harness);

    type_into(
        &mut fx.harness,
        "Repository URL",
        "https://example.com/some/repo.git",
    );
    fx.harness.get_by_label("Clone").click();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "no folder picker available");
    let s = fx.harness.state();
    assert!(s.show_welcome(), "a failed pick must not enter the shell");
    assert_eq!(
        s.ui.welcome_clone_url, "https://example.com/some/repo.git",
        "the typed URL is kept so the user can retry"
    );
}

#[test]
fn cancelling_the_clone_folder_pick_surfaces_a_toast_and_keeps_welcome() {
    let mut fx = fixture_with_picker(|| None);
    settle(&mut fx.harness);

    type_into(
        &mut fx.harness,
        "Repository URL",
        "https://example.com/some/repo.git",
    );
    fx.harness.get_by_label("Clone").click();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "no folder selected");
    let s = fx.harness.state();
    assert!(s.show_welcome());
    assert!(s.multi.roots.is_empty());
}

#[test]
fn folder_picker_seam_is_only_invoked_behind_user_initiated_flows() {
    static PICKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut fx = fixture_with_picker(|| {
        PICKS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        None
    });

    // Merely rendering Welcome — launch, settle, File → Welcome round-trip —
    // must never open a native dialog.
    settle(&mut fx.harness);
    settle(&mut fx.harness);
    assert_eq!(
        PICKS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "rendering the Welcome screen must not invoke the picker seam"
    );

    // An explicit user action is what triggers it.
    fx.harness.get_by_label("Open Project").click();
    settle(&mut fx.harness);
    assert!(
        PICKS.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "clicking Open Project must go through the picker seam exactly once per click"
    );
    assert!(
        fx.harness.state().show_welcome(),
        "a cancelled pick leaves the user on Welcome"
    );
}
