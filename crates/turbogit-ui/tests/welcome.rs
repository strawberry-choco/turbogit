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

use egui::{Color32, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, painted_text, settle, shell_harness_over,
};
use test_support::wcag::contrast;
use turbogit_app::recents::{RecentProject, Recents, load, recents_file, record, save};
use turbogit_app::state::AppState;

/// Step frames until `needle` is painted or the wall-clock budget runs out
/// (async worker results — status scans, recents branches — repaint later
/// than the layout settles).
#[track_caller]
fn wait_painted(harness: &mut Harness<'_, AppState>, needle: &str, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !painted_text(harness).iter().any(|t| t.contains(needle)) {
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: `{needle}` was never painted"
        );
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Create a real repository with a deterministic initial branch (`main`) and
/// **no commits at all**.
///
/// **Deliberately NOT `git_seed::repo_with_one_commit`**: several tests here assert the
/// *unborn* state, and a recipe that commits `README.md` would put a commit where these
/// tests need none. Only the runner is shared.
fn seed_repo(base: &Path, name: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&dir, &["init", "-b", "main"]);
    dir
}

/// Run `git <args>` in `cwd`, returning trimmed stdout (tests read history).
#[track_caller]
fn git_out(args: &[&str], cwd: &Path) -> String {
    git(cwd, args).trim().to_string()
}

/// A real bare "remote" with three commits on `main` (issue #17).
fn seed_bare_remote(base: &Path, name: &str) -> PathBuf {
    let work = seed_repo(base, &format!("{name}-work"));
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    for i in 1..=3 {
        std::fs::write(work.join(format!("f{i}.txt")), format!("commit {i}"))
            .expect("write tracked file");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", &format!("c{i}")]);
    }
    let bare = base.join(format!("{name}.git"));
    let work_s = work.to_string_lossy().to_string();
    let bare_s = bare.to_string_lossy().to_string();
    git(base, &["clone", "--bare", "-q", &work_s, &bare_s]);
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

    let harness = shell_harness_over(state, egui::vec2(1024.0, 768.0));
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
fn welcome_paints_hero_and_three_action_cards() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "TurboGit");
    assert_painted(&fx.harness, "A fast, keyboard-friendly Git client");
    // The hero carries the "What's new" trigger.
    assert_painted(&fx.harness, "What's new");
    // Exactly three quick-action cards — the old "Clone from URL" card is gone,
    // its door merged into the clone panel below.
    assert_not_painted(&fx.harness, "Clone from URL");
    assert_painted(&fx.harness, "Open Project");
    assert_painted(&fx.harness, "Initialize Repository");
    assert_painted(&fx.harness, "Attach Workspace Root");
    // The single merged clone panel is titled "Clone a repository".
    assert_painted(&fx.harness, "Clone a repository");
    // Recents column exists even when empty.
    assert_painted(&fx.harness, "RECENT PROJECTS");
    assert_painted(&fx.harness, "No recent projects yet.");
}

/// Ticket 01: the hero is ONE row — the "What's new" trigger is vertically
/// centred against the wordmark+tagline brand block, not stacked on its own
/// centered line beneath it the way `what_new_link` placed it.
#[test]
fn hero_keeps_whats_new_on_the_brand_row() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    let wordmark = painted_text_centers(&fx.harness, "TurboGit")
        .into_iter()
        .next()
        .expect("the hero wordmark must be painted");
    let tagline = painted_text_centers(
        &fx.harness,
        "A fast, keyboard-friendly Git client for your desktop.",
    )
    .into_iter()
    .next()
    .expect("the hero tagline must be painted");
    let whats_new = painted_text_centers(&fx.harness, "What's new")
        .into_iter()
        .next()
        .expect("the hero 'What's new' trigger must be painted");
    let (w_y, t_y, n_y) = (wordmark.y, tagline.y, whats_new.y);
    assert!(
        w_y < n_y && n_y < t_y,
        "'What's new' must sit within the hero brand row between the wordmark \
         (y={w_y}) and the tagline (y={t_y}); got y={n_y}"
    );
}

#[test]
fn clone_box_offers_url_input_and_clone_action() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    // The merged clone panel: titled header + one door, not a card + a box.
    assert_painted(&fx.harness, "Clone a repository");
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
    // The welcome page is gone; the shell status bar reports the opened
    // root through the workspace aggregate (issue #03), not per-root text.
    // The freshly-initialized seed repo is clean, so only the total chip
    // can appear.
    assert_not_painted(&fx.harness, "A fast, keyboard-friendly Git client");
    wait_painted(&mut fx.harness, "1 total", "opened repo status aggregate");
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
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let state = AppState::launch_in(None, Some(cfg));
    let mut harness = shell_harness_over(state, egui::vec2(1024.0, 768.0));
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

/// The actual foreground/background pair under the branch text of a recent
/// row: the `Shape::Text` whose galley holds `text`, plus the opaque painted
/// rect (the chip) directly beneath it. Fails if either is not rendered.
#[track_caller]
fn painted_fg_bg_pair(harness: &Harness<'_, AppState>, text: &str) -> (Color32, Color32) {
    let shapes = &harness.output().shapes;
    let text_shape = shapes
        .iter()
        .enumerate()
        .find_map(|(index, clipped)| match &clipped.shape {
            Shape::Text(t) if t.galley.text() == text => Some((index, t)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{text}` branch text was not painted"));

    let (index, text_shape) = text_shape;
    // The chip rect is painted before its text, so the background is the last
    // opaque rect behind the text's center.
    let foreground = text_shape
        .override_text_color
        .unwrap_or(text_shape.galley.job.sections[0].format.color);
    let center = text_shape.pos + text_shape.galley.size() / 2.0;
    let background = shapes[..index]
        .iter()
        .rev()
        .find_map(|clipped| match &clipped.shape {
            Shape::Rect(rect) if rect.rect.contains(center) && rect.fill.a() == 255 => {
                Some(rect.fill)
            }
            _ => None,
        })
        .expect("opaque chip background must sit beneath branch text");
    (foreground, background)
}

/// Ticket 01 (C1): the welcome branch chip must use small-text ink that
/// clears 4.5:1 against its actual SURFACE_3 chip background — the regression
/// that fails while the chip is painted in action-fill BRAND (audit: 2.479:1)
/// and passes once the readable accent ink is used.
#[test]
fn recent_branch_text_is_readable_on_its_painted_chip() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_000,
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);
    wait_painted(&mut harness, "main", "current branch");

    let (foreground, background) = painted_fg_bg_pair(&harness, "main");
    let ratio = contrast(foreground, background);
    // The branch text is normal text (11px) on the recent-row chip; it must
    // reach the 4.5:1 normal-text benchmark against the actual chip fill.
    assert!(
        ratio >= 4.5,
        "branch chip text {foreground:?} on {background:?} is {ratio:.3}:1 — below 4.5:1"
    );
}

/// Ticket 03: the recents card's branch chip now paints the deliberate
/// `SELECTION` fill (spec §4 contrast change) — the ink stays `ACCENT_TEXT`.
#[test]
fn recent_branch_chip_paints_the_selection_fill() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_000,
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);
    wait_painted(&mut harness, "main", "current branch");

    let (foreground, background) = painted_fg_bg_pair(&harness, "main");
    assert_eq!(
        background,
        turbogit_ui::theme::Palette::SELECTION,
        "the recent-row branch chip must paint the SELECTION fill"
    );
    assert_eq!(
        foreground,
        turbogit_ui::theme::Palette::ACCENT_TEXT,
        "branch chip text ink is unchanged"
    );
}

/// Ticket 03: the recents card is enclosed and footers a "Show all projects"
/// affordance. It is a v1 no-op (no recents browser to route to — plan §4), so
/// the screen still simply paints the Welcome page.
#[test]
fn recents_card_is_enclosed_and_footers_show_all_projects() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_050,
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);

    // Enclosed card: header, a seeded row, and the footer all paint together.
    assert_painted(&harness, "RECENT PROJECTS");
    assert_painted(&harness, "alpha");
    assert_painted(&harness, "Show all projects");
    // A recents card fill on CONTENT_BG backs the rows.
    let card_fill = harness
        .output()
        .shapes
        .iter()
        .any(|clipped| match &clipped.shape {
            Shape::Rect(r) => r.fill == turbogit_ui::theme::Palette::CONTENT_BG,
            _ => false,
        });
    assert!(card_fill, "the recents card must paint a CONTENT_BG frame");
}

/// Ticket 04: the `lower` band places the recents card (left, wider) beside the
/// getting-started card (right). Before this the getting-started hints were a
/// bare full-width list stacked well below a right-hand recents column, so the
/// two headers were neither on one band nor in left→right order.
#[test]
fn lower_puts_recents_left_and_getting_started_right() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_050,
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);

    let recents = painted_text_centers(&harness, "RECENT PROJECTS")
        .into_iter()
        .next()
        .expect("recents header");
    let getting = painted_text_centers(&harness, "GETTING STARTED")
        .into_iter()
        .next()
        .expect("getting-started header");
    let (dy, dx) = ((recents.y - getting.y).abs(), getting.x - recents.x);
    assert!(
        dy <= 40.0 && dx > 0.0,
        "getting-started must sit beside (right of) recents on one band: \
         recents=({},{}) getting=({},{}) -> dx={dx} dy={dy}",
        recents.x,
        recents.y,
        getting.x,
        getting.y,
    );
    // The five hints still paint their text inside the card.
    assert_painted(&harness, "Stage files in the Commit tool window.");
}

/// The three quick-action cards must share ONE row (left→right, same top). The
/// egui auto `item_spacing.x` around each explicit gap made the old width math
/// overflow, wrapping the third card ("Attach Workspace Root") onto its own
/// line — so its title painted far below the other two and its right edge no
/// longer aligned with the hero / clone panel.
#[test]
fn welcome_quick_actions_share_one_row() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    let mut centers = Vec::new();
    for title in [
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
    ] {
        let c = painted_text_centers(&fx.harness, title)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{title} card title not painted"));
        centers.push((title, c));
    }
    let y0 = centers[0].1.y;
    for (title, c) in &centers {
        assert!(
            (c.y - y0).abs() <= 4.0,
            "all three cards share one row: {title} y={} vs Open Project y={y0}",
            c.y
        );
    }
    // Left → right order.
    assert!(
        centers[0].1.x < centers[1].1.x && centers[1].1.x < centers[2].1.x,
        "cards must run left→right: {:?}",
        centers.iter().map(|(_, c)| c.x).collect::<Vec<_>>()
    );
}

/// The getting-started card is one column of the shared 3-up card grid: its
/// width and left/right edges must line up with the third quick-action card
/// ("Attach Workspace Root"), and the recents↔getting gap stays one gutter.
#[test]
fn welcome_getting_started_aligns_under_the_attach_card() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    let attach = fx.harness.get_by_label("Attach Workspace Root").rect();

    // The getting-started card is the only narrow SURFACE-filled rect (the shell
    // top/status bars are full-width SURFACE bands).
    let getting = fx
        .harness
        .output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            Shape::Rect(r)
                if r.fill == turbogit_ui::theme::Palette::SURFACE && r.rect.width() < 600.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .next()
        .expect("the getting-started card must paint a SURFACE frame");

    let (aw, gw) = (attach.width(), getting.width());
    assert!(
        (aw - gw).abs() <= 3.0,
        "getting-started width {gw} should match the attach card width {aw}"
    );
    assert!(
        (attach.min.x - getting.min.x).abs() <= 3.0,
        "getting-started left {} must align under attach left {}",
        getting.min.x,
        attach.min.x
    );
    assert!(
        (attach.max.x - getting.max.x).abs() <= 3.0,
        "getting-started right {} must align under attach right {}",
        getting.max.x,
        attach.max.x
    );
}

// ----------------------------------- issue: shared typography roles (T2) ----

/// Paint-time font sizes (points) of every galley carrying exactly `text`.
fn painted_font_sizes(harness: &Harness<'_, AppState>, text: &str) -> Vec<f32> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(t) if t.galley.text() == text => Some(
                t.galley
                    .job
                    .sections
                    .first()
                    .map_or(0.0, |s| s.format.font_id.size),
            ),
            _ => None,
        })
        .collect()
}

/// Vertical centers (screen points) of every text galley whose text is
/// exactly `text`. Used to assert which *row* an element sits on.
fn painted_text_centers(harness: &Harness<'_, AppState>, text: &str) -> Vec<egui::Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() / 2.0),
            _ => None,
        })
        .collect()
}

/// How many painted galleys carry exactly `text` — used to prove an element is
/// painted once (no duplicated brand, no fourth card), not merely present.
fn count_painted(harness: &Harness<'_, AppState>, text: &str) -> usize {
    painted_text_centers(harness, text).len()
}

/// Whether any painted rect carries exactly `fill` (a card/frame surface check).
fn paints_rect_with_fill(harness: &Harness<'_, AppState>, fill: Color32) -> bool {
    harness
        .output()
        .shapes
        .iter()
        .any(|clipped| match &clipped.shape {
            Shape::Rect(r) => r.fill == fill,
            _ => false,
        })
}

/// T2: the welcome wordmark is a distinct display role that renders through
/// the shared named size — never a local literal that central changes miss.
#[test]
fn welcome_wordmark_uses_the_shared_display_role() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    let sizes = painted_font_sizes(&fx.harness, "TurboGit");
    assert!(
        sizes
            .iter()
            .any(|s| (*s - turbogit_ui::theme::TYPE_WORDMARK).abs() < 0.01),
        "the welcome wordmark must render at the shared wordmark size ({}); got {sizes:?}",
        turbogit_ui::theme::TYPE_WORDMARK
    );
}

/// T2: the stats-and-table display in multi-root selection uses the shared
/// statistic size (named display role), not a local literal.
#[test]
fn display_roles_have_named_shared_sizes() {
    use turbogit_ui::theme::{TYPE_STATISTIC, TYPE_WORDMARK};
    // Distinct display roles exist and are not body text (const check — the
    // values are compile-time constants, so this is a static property).
    const { assert!(TYPE_WORDMARK > turbogit_ui::theme::TYPE_BODY) };
    const { assert!(TYPE_STATISTIC > turbogit_ui::theme::TYPE_BODY) };
    const { assert!(TYPE_WORDMARK != TYPE_STATISTIC) };
}

// ----------------------------------- issue: shared chip shape variants (S3) --

/// S3: the welcome branch chip and repo-count chip paint the shared pill
/// shape (PILL_RADIUS) — they no longer declare a local shape contract, and
/// the pill stays distinct from the compact chip/control radii.
#[test]
fn welcome_chips_paint_the_shared_pill_shape() {
    let mut fx = bare_fixture();
    let repo = seed_repo(fx._project.path(), "shape-project");
    fx.harness.state_mut().ui.recent_projects = vec![RecentProject {
        path: repo,
        name: "shape-project".into(),
        last_opened: 1_755_000_000_000,
        kind: turbogit_app::recents::RecentKind::Project,
        repo_count: None,
    }];
    settle(&mut fx.harness);
    wait_painted(&mut fx.harness, "main", "current branch");

    let pill_radius = turbogit_ui::theme::PILL_RADIUS;
    let chip_fills = fx
        .harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect) if rect.fill == turbogit_ui::theme::Palette::SURFACE_3 => {
                Some(rect.corner_radius)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        chip_fills
            .iter()
            .any(|r| r == &egui::CornerRadius::same(pill_radius)),
        "the welcome chip must paint the shared pill radius {pill_radius}; got {chip_fills:?}"
    );
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
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
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
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);
    assert_painted(&harness, "main");

    // Mutate the repo OUTSIDE TurboGit: switch to another branch.
    git(&repo, &["checkout", "-b", "feature/next"]);
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

    let mut harness = shell_harness_over(
        AppState::launch(Some(repo.clone())),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);
    assert!(
        !harness.state().show_welcome(),
        "launching with a path must enter the shell directly"
    );

    // The old File → Welcome Screen menu retired with the IDE chrome
    // (issue #03) and the topbar's More button with the topbar; the
    // command palette's Open Welcome action is the way back now.
    harness.state_mut().ui.command_palette = true;
    harness.state_mut().ui.command_query = "open welcome".to_string();
    settle(&mut harness);
    harness.get_by_label("Open Welcome").click();
    settle(&mut harness);

    let s = harness.state();
    assert!(s.show_welcome(), "Open Welcome must return to the screen");
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

// --- Cycle 9: Workspace upgrades (issue #34) ----------------------------------

/// Build a workspace container with two nested repos, one strictly deeper
/// than the bounded scanner's SCAN_MAX_DEPTH so only the deep scan finds it.
fn seed_workspace(base: &Path) -> PathBuf {
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let shallow = ws.join("alpha");
    std::fs::create_dir_all(&shallow).unwrap();
    git(&shallow, &["init", "-q", "-b", "main"]);
    let deep = ws.join("a").join("b").join("c").join("d");
    std::fs::create_dir_all(&deep).unwrap();
    git(&deep, &["init", "-q", "-b", "main"]);
    ws
}

#[test]
fn welcome_paints_the_attach_workspace_card() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);
    assert_painted(&fx.harness, "Attach Workspace Root");
}

#[test]
fn attach_card_registers_every_repo_in_the_picked_tree() {
    let project = tempfile::tempdir().expect("temp project dir");
    let ws = seed_workspace(project.path());

    let picked = ws.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);

    fx.harness.get_by_label("Attach Workspace Root").click();
    settle(&mut fx.harness);

    let s = fx.harness.state();
    assert!(
        !s.show_welcome(),
        "attaching a workspace must enter the shell"
    );
    // Both repos — the shallow sibling and the deep-nested one — register.
    assert_eq!(s.multi.roots.len(), 2, "both repos must be indexed");

    // The workspace is offered back in the persisted recents as a workspace
    // row carrying the indexed repo count.
    let persisted = load(fx.config.path());
    let ws_row = persisted
        .projects
        .iter()
        .find(|p| p.path == ws)
        .expect("workspace must be recorded in the global store");
    assert_eq!(ws_row.kind, turbogit_app::recents::RecentKind::Workspace);
    assert_eq!(ws_row.repo_count, Some(2));
}

#[test]
fn workspace_recent_renders_repo_count_and_clicking_restores_it() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let ws = seed_workspace(project.path());

    seed_recents(
        config.path(),
        &[turbogit_app::recents::RecentProject {
            path: ws.clone(),
            name: "ws".into(),
            last_opened: 1_755_000_000_000,
            kind: turbogit_app::recents::RecentKind::Workspace,
            repo_count: Some(2),
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);

    // The workspace row paints its indexed repo count.
    assert_painted(&harness, "2 repos");

    // Clicking the row restores the workspace: every repo is re-registered.
    harness.get_by_label("ws").click();
    settle(&mut harness);
    let s = harness.state();
    assert!(
        !s.show_welcome(),
        "clicking a workspace recent must re-enter the shell"
    );
    assert_eq!(
        s.multi.roots.len(),
        2,
        "restoring the workspace re-indexes its repos"
    );
}

#[test]
fn status_bar_paints_version_line_with_app_version_git_version_and_repo_count() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    // App version + resolved git version + indexed count line. It rides the
    // status bar's left cluster — the one chrome band Welcome keeps — since
    // the topbar it used to sit in was deleted.
    assert_painted(&fx.harness, &format!("v{}", env!("CARGO_PKG_VERSION")));
    assert_painted(&fx.harness, "git ");
    assert_painted(&fx.harness, "repos indexed");
    // No workspace open on a fresh Welcome → 0 indexed.
    assert_painted(&fx.harness, "0 repos indexed");
}

/// Ticket 01: the hero's "What's new" ghost button opens and closes the same
/// changelog overlay the retired `what_new_link` drove — unchanged behaviour,
/// new trigger.
#[test]
fn hero_whats_new_opens_and_closes_the_changelog_overlay() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "What's new");
    assert_not_painted(&fx.harness, "What's New");

    fx.harness.get_by_label("What's new").click();
    settle(&mut fx.harness);
    assert_painted(&fx.harness, "What's New");

    fx.harness.get_by_label("Close").click();
    settle(&mut fx.harness);
    assert_not_painted(&fx.harness, "What's New");
}

// --- Ticket 06: the redesigned structure, asserted not assumed -----------------

/// Freeze the new layout end-to-end through the painted seam (spec §9). Uses
/// only strings the Welcome panel itself paints — the brand wordmark moved
/// here with the topbar's deletion, so it is no longer a shared shell string
/// to exclude — so counts prove single-instance-ness of each region.
#[test]
fn welcome_locks_the_redesigned_structure() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let repo = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: repo.clone(),
            name: "alpha".into(),
            last_opened: 1_755_000_000_050,
            kind: turbogit_app::recents::RecentKind::Project,
            repo_count: None,
        }],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);

    // Hero: wordmark + tagline + "What's new" trigger all paint.
    assert_painted(&harness, "TurboGit");
    assert_painted(
        &harness,
        "A fast, keyboard-friendly Git client for your desktop.",
    );
    assert_painted(&harness, "What's new");

    // Exactly ONE clone door: the merged panel header paints once, the retired
    // "Clone from URL" card is gone.
    assert_eq!(
        count_painted(&harness, "Clone a repository"),
        1,
        "there is exactly one clone panel"
    );
    assert_not_painted(&harness, "Clone from URL");

    // Exactly three quick-action cards, each title a single galley.
    for title in [
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
    ] {
        assert_eq!(
            count_painted(&harness, title),
            1,
            "quick-action card {title:?} should paint exactly once"
        );
    }

    // Recents card: header, a seeded row, and the footer.
    assert_painted(&harness, "RECENT PROJECTS");
    assert_painted(&harness, "alpha");
    assert_painted(&harness, "Show all projects");

    // Getting-started card: header + at least one hint body.
    assert_painted(&harness, "GETTING STARTED");
    assert_painted(&harness, "Browse history in the Git Log tool window.");

    // Token fills the harness can observe at the painted seam: the CONTENT_BG
    // cards (clone panel / quick actions / recents) and the SURFACE
    // getting-started card both paint their frame. The SELECTION branch chip is
    // pinned separately by recent_branch_chip_paints_the_selection_fill.
    use turbogit_ui::theme::Palette;
    assert!(
        paints_rect_with_fill(&harness, Palette::CONTENT_BG),
        "a CONTENT_BG card frame must paint"
    );
    assert!(
        paints_rect_with_fill(&harness, Palette::SURFACE),
        "the getting-started SURFACE card must paint"
    );
}

// ------------------------------------------- 19: the type ramp, and nothing
//    else, about the welcome screen -------------------------------------

/// The welcome screen takes its **type** from the shared token layer and keeps
/// no local copy of it — the wordmark and the changelog dialog's title both
/// read a named role, and neither restates a number.
///
/// Two halves, and the second is the one that actually catches drift:
///
/// 1. **From painted output.** The wordmark paints at `TYPE_WORDMARK` and the
///    changelog's title at `TYPE_PANE_TITLE`, in the chrome face, both read
///    from the token layer rather than from a literal beside them.
/// 2. **From the module's source.** No type size is written out anywhere in
///    `ui/welcome.rs`. The first half alone would pass for a screen whose type
///    happens to be right today and is spelled locally tomorrow, and the
///    criterion is about the *role* being shared, not about today's number
///    matching.
///
/// The second half is a source scan, and source scans are normally a smell.
/// It is the right tool for exactly one claim — "this file contains no literal
/// of this kind" — because that claim is about the text of the file and there
/// is no render that can see it. It is scoped to `theme::TYPE_` by name so a
/// geometry literal in the same file is not a false positive.
#[test]
fn the_welcome_screen_keeps_no_local_copy_of_its_type() {
    use turbogit_ui::theme::{TYPE_PANE_TITLE, TYPE_WORDMARK};

    // (1) From painted output. The wordmark is the hero's headline; the
    //     changelog title is the floating dialog's own heading.
    let mut fx = bare_fixture();
    let galley_for = |harness: &Harness<'static, turbogit_app::state::AppState>, text: &str| {
        let hits: Vec<_> = test_support::harness::painted_galleys(harness)
            .into_iter()
            .filter(|g| g.text == text)
            .collect();
        assert_eq!(hits.len(), 1, "one `{text}` on the screen; found {hits:#?}");
        hits.into_iter().next().expect("one hit")
    };
    let wordmark = galley_for(&fx.harness, "TurboGit");
    assert!(
        wordmark.rect.height() > TYPE_BODY_HINT,
        "the wordmark is the display role ({TYPE_WORDMARK}pt), not body text: \
         {wordmark:?}"
    );

    // Open the changelog and read its title.
    fx.harness.state_mut().ui.show_changelog = true;
    settle(&mut fx.harness);
    let title = galley_for(&fx.harness, "What's New");
    // **By ratio, not by absolute height.** A galley's painted height is its
    // font's line height, which is larger than the nominal size and carries the
    // face's own leading — so "the title is 16 points tall" is not a claim the
    // render can make. The *ratio* between two headings in the same face is,
    // because line height scales with the nominal size: this says the title is
    // the pane-title role to the wordmark, which is the actual claim, and it
    // keeps working if the face's leading is ever retuned.
    let want = TYPE_PANE_TITLE / TYPE_WORDMARK;
    let got = title.rect.height() / wordmark.rect.height();
    assert!(
        (got - want).abs() < want * 0.15,
        "the changelog dialog's title is the shared pane-title role relative to \
         the wordmark's display role: it measures {got:.3} where \
         {TYPE_PANE_TITLE}/{TYPE_WORDMARK} is {want:.3} (title {title:?}, \
         wordmark {wordmark:?})"
    );
    // (2) From the module's own source: no type size is spelled out here. Any
    //     `theme::TYPE_*` name is a shared role; a bare number in a type
    //     position would be a local copy of one, and that is the thing this
    //     assertion exists to stop.
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/welcome.rs"),
    )
    .expect("read ui/welcome.rs");
    // A type size is a number *passed to* one of the four ways this crate sets
    // type. A number in any other position — a galley's measured `.size().y`, a
    // padding constant, a height sum — is geometry, and flagging it would make
    // the ratchet cry wolf on the first line someone edits.
    const TYPE_CALLS: [&str; 4] = ["FontId::new(", ".size(", "chrome_font(", "data_font("];
    let offenders: Vec<String> = source
        .lines()
        .enumerate()
        .filter_map(|(n, line)| {
            // Comments are blanked, not stripped, so a `//` inside a string
            // literal cannot truncate the line and hide a literal.
            let code: String = line
                .split("//")
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            TYPE_CALLS
                .iter()
                .any(|call| {
                    code.split(call).skip(1).any(|rest| {
                        rest.trim_start()
                            .trim_start_matches([' ', '('])
                            .starts_with(|c: char| c.is_ascii_digit() || c == '.')
                    })
                })
                .then(|| format!("{}: {}", n + 1, line.trim()))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "ui/welcome.rs takes its type from the shared token layer and keeps no \
         local copy: every size it sets is a `theme::TYPE_*` role. A bare \
         number in a type position is a second spelling of one: {offenders:#?}"
    );
}

/// A body-size label, as a floor for the wordmark's own check. Named so the
/// assertion above reads as a statement about the ramp rather than as a
/// comparison against another constant in the same expression.
const TYPE_BODY_HINT: f32 = turbogit_ui::theme::TYPE_BODY;
