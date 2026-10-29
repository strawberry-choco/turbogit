use egui::{Color32, Shape};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, painted_text, settle, shell_harness_over,
};
use test_support::wcag::contrast;
use turbogit_app::recents::{RecentProject, Recents, load, recents_file, record, save};
use turbogit_app::state::AppState;

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

fn seed_repo(base: &Path, name: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&dir, &["init", "-b", "main"]);
    dir
}

#[track_caller]
fn git_out(args: &[&str], cwd: &Path) -> String {
    git(cwd, args).trim().to_string()
}

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
    _project: tempfile::TempDir,
    config: tempfile::TempDir,
}

fn fixture_with_picker(pick: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) -> Fixture {
    fixture_inner(Some(Box::new(pick)))
}

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

#[test]
fn welcome_paints_hero_and_three_action_cards() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "TurboGit");
    assert_painted(&fx.harness, "A fast, keyboard-friendly Git client");
    assert_painted(&fx.harness, "What's new");
    assert_not_painted(&fx.harness, "Clone from URL");
    assert_painted(&fx.harness, "Open Project");
    assert_painted(&fx.harness, "Initialize Repository");
    assert_painted(&fx.harness, "Attach Workspace Root");
    assert_painted(&fx.harness, "Clone a repository");
    assert_painted(&fx.harness, "RECENT PROJECTS");
    assert_painted(&fx.harness, "No recent projects yet.");
}

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

    assert_painted(&fx.harness, "Clone a repository");
    assert_painted(&fx.harness, "Repository URL");
    assert_painted(&fx.harness, "Clone");
    assert_painted(&fx.harness, "Shallow clone");
    assert_painted(&fx.harness, "GETTING STARTED");
}

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
    let sep = std::path::MAIN_SEPARATOR;
    let texts = painted_text(&harness);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("alpha") && t != "alpha" && t.contains(sep)),
        "recent row must paint the project path; painted text:\n{texts:#?}"
    );
    assert_painted(&harness, "Last opened");
    assert_painted(&harness, "main");
}

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
    assert!(
        ratio >= 4.5,
        "branch chip text {foreground:?} on {background:?} is {ratio:.3}:1 — below 4.5:1"
    );
}

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

    assert_painted(&harness, "RECENT PROJECTS");
    assert_painted(&harness, "alpha");
    assert_painted(&harness, "Show all projects");
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
    assert_painted(&harness, "Stage files in the Commit tool window.");
}

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
    assert!(
        centers[0].1.x < centers[1].1.x && centers[1].1.x < centers[2].1.x,
        "cards must run left→right: {:?}",
        centers.iter().map(|(_, c)| c.x).collect::<Vec<_>>()
    );
}

#[test]
fn welcome_getting_started_aligns_under_the_attach_card() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    let attach = fx.harness.get_by_label("Attach Workspace Root").rect();

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

fn count_painted(harness: &Harness<'_, AppState>, text: &str) -> usize {
    painted_text_centers(harness, text).len()
}

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

#[test]
fn display_roles_have_named_shared_sizes() {
    use turbogit_ui::theme::{TYPE_STATISTIC, TYPE_WORDMARK};
    const { assert!(TYPE_WORDMARK > turbogit_ui::theme::TYPE_BODY) };
    const { assert!(TYPE_STATISTIC > turbogit_ui::theme::TYPE_BODY) };
    const { assert!(TYPE_WORDMARK != TYPE_STATISTIC) };
}

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

#[test]
fn a_recent_row_offers_a_remove_control_that_drops_only_that_row() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let alpha = seed_repo(project.path(), "alpha");
    let beta = seed_repo(project.path(), "beta");

    seed_recents(
        config.path(),
        &[
            RecentProject {
                path: alpha.clone(),
                name: "alpha".into(),
                last_opened: 1_755_000_000_000,
                kind: turbogit_app::recents::RecentKind::Project,
                repo_count: None,
            },
            RecentProject {
                path: beta.clone(),
                name: "beta".into(),
                last_opened: 1_754_000_000_000,
                kind: turbogit_app::recents::RecentKind::Project,
                repo_count: None,
            },
        ],
    );

    let cfg = config.path().to_path_buf();
    let mut harness = shell_harness_over(
        AppState::launch_in(None, Some(cfg)),
        egui::vec2(1024.0, 768.0),
    );
    settle(&mut harness);
    assert_painted(&harness, "alpha");
    assert_painted(&harness, "beta");

    harness.get_by_label("Remove recent project alpha").click();
    settle(&mut harness);

    let buttons: Vec<String> = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter_map(|n| n.accesskit_node().label().map(|l| l.to_string()))
        .collect();
    assert!(
        !buttons.iter().any(|l| l == "alpha"),
        "the removed row's own control is gone; buttons: {buttons:#?}"
    );
    assert!(
        buttons.contains(&"beta".to_string()),
        "the neighbour keeps its row; buttons: {buttons:#?}"
    );
    let removes: Vec<_> = buttons
        .iter()
        .filter(|l| l.starts_with("Remove recent project"))
        .collect();
    assert_eq!(
        removes,
        vec!["Remove recent project beta"],
        "one remove control per listed row, and only the removed row's is gone"
    );

    assert!(
        harness.state().show_welcome(),
        "removing a row must not open it — the control and the row are one click apart"
    );

    let listed: Vec<_> = harness
        .state()
        .ui
        .recent_projects
        .iter()
        .map(|p| p.path.clone())
        .collect();
    assert_eq!(listed, vec![beta], "exactly one row left");
}

#[test]
fn removing_a_recent_row_says_which_project_it_dropped() {
    let project = tempfile::tempdir().expect("temp project dir");
    let config = tempfile::tempdir().expect("temp config dir");
    let alpha = seed_repo(project.path(), "alpha");

    seed_recents(
        config.path(),
        &[RecentProject {
            path: alpha.clone(),
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

    harness.get_by_label("Remove recent project alpha").click();
    settle(&mut harness);

    assert_painted(&harness, "RECENT PROJECTS");
    assert_painted(&harness, "No recent projects yet.");

    assert!(
        turbogit_app::recents::load(config.path())
            .projects
            .is_empty(),
        "the removed row must not come back on the next launch"
    );
    assert!(
        alpha.is_dir(),
        "removing a recent must not touch the project"
    );

    let toast = harness
        .state()
        .ui
        .toast
        .as_ref()
        .expect("the removal reports itself");
    assert!(
        toast.message.contains("alpha"),
        "the notice names the project the list no longer shows; got {:?}",
        toast.message
    );
}

#[test]
fn the_remove_control_sits_in_the_row_band_past_the_chips() {
    let mut fx = bare_fixture();
    let repo = seed_repo(fx._project.path(), "zoned");
    fx.harness.state_mut().ui.recent_projects = vec![RecentProject {
        path: repo,
        name: "zoned".into(),
        last_opened: 1_755_000_000_000,
        kind: turbogit_app::recents::RecentKind::Project,
        repo_count: None,
    }];
    settle(&mut fx.harness);
    wait_painted(&mut fx.harness, "main", "current branch");

    let button = fx
        .harness
        .get_by_label("Remove recent project zoned")
        .rect();
    let row = fx.harness.get_by_label("zoned").rect();
    let name = painted_text_centers(&fx.harness, "zoned")
        .into_iter()
        .next()
        .expect("the row's name paints");

    assert!(
        button.max.x > row.max.x,
        "the control sits past the row's own target: button {button:?}, row {row:?}"
    );
    assert!(
        button.center().y > row.center().y - 1.0 && button.center().y < row.center().y + 1.0,
        "the control is vertically centred on its row: button {button:?}, row {row:?}"
    );
    assert!(
        button.min.x > name.x,
        "the control never covers the name it removes: name at x={}, button {button:?}",
        name.x
    );
    let chip = painted_text_centers(&fx.harness, "main")
        .into_iter()
        .next()
        .expect("the branch chip paints");
    assert!(
        chip.x < button.min.x,
        "the branch chip ends before the control: chip x={}, button {button:?}",
        chip.x
    );
}

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

    git(&repo, &["checkout", "-b", "feature/next"]);
    settle(&mut harness);
    assert_painted(&harness, "main");
    assert_not_painted(&harness, "feature/next");

    harness.state_mut().invalidate_welcome_branches();
    settle(&mut harness);
    assert_painted(&harness, "feature/next");
    assert_not_painted(&harness, "main");
}

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

    let state = AppState::launch(Some(repo));
    assert!(!state.show_welcome());
    assert_eq!(state.multi.roots.len(), 1);

    let config = tempfile::tempdir().expect("temp config dir");
    let state = AppState::launch_in(None, Some(config.path().to_path_buf()));
    assert!(state.show_welcome());
    assert!(state.multi.roots.is_empty());
}

#[test]
fn recents_store_roundtrips_upserts_sorts_and_caps() {
    let config = tempfile::tempdir().expect("temp config dir");

    assert!(load(config.path()).projects.is_empty());

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

    record(config.path(), &a);
    let recents = load(config.path());
    assert_eq!(recents.projects.len(), 2, "upsert must not duplicate");
    assert_eq!(recents.projects[0].name, "alpha", "re-opened moves to top");

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

    let file = recents_file(config.path());
    std::fs::write(&file, "not ron at all {{{").unwrap();
    assert!(load(config.path()).projects.is_empty());
}

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

    assert!(
        dest.join(".git").exists(),
        "clone must land at <picked parent>/origin"
    );
    assert_eq!(
        git_out(&["rev-list", "--count", "HEAD"], &dest),
        "3",
        "a full clone carries the remote's entire history"
    );

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

    settle(&mut fx.harness);
    settle(&mut fx.harness);
    assert_eq!(
        PICKS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "rendering the Welcome screen must not invoke the picker seam"
    );

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
    assert_eq!(s.multi.roots.len(), 2, "both repos must be indexed");

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

    assert_painted(&harness, "2 repos");

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

    assert_painted(&fx.harness, &format!("v{}", env!("CARGO_PKG_VERSION")));
    assert_painted(&fx.harness, "git ");
    assert_painted(&fx.harness, "repos indexed");
    assert_painted(&fx.harness, "0 repos indexed");
}

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

    assert_painted(&harness, "TurboGit");
    assert_painted(
        &harness,
        "A fast, keyboard-friendly Git client for your desktop.",
    );
    assert_painted(&harness, "What's new");

    assert_eq!(
        count_painted(&harness, "Clone a repository"),
        1,
        "there is exactly one clone panel"
    );
    assert_not_painted(&harness, "Clone from URL");

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

    assert_painted(&harness, "RECENT PROJECTS");
    assert_painted(&harness, "alpha");
    assert_painted(&harness, "Show all projects");

    assert_painted(&harness, "GETTING STARTED");
    assert_painted(&harness, "Browse history in the Git Log tool window.");

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

#[test]
fn the_welcome_screen_keeps_no_local_copy_of_its_type() {
    use turbogit_ui::theme::{TYPE_PANE_TITLE, TYPE_WORDMARK};

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

    fx.harness.state_mut().ui.show_changelog = true;
    settle(&mut fx.harness);
    let title = galley_for(&fx.harness, "What's New");
    let want = TYPE_PANE_TITLE / TYPE_WORDMARK;
    let got = title.rect.height() / wordmark.rect.height();
    assert!(
        (got - want).abs() < want * 0.15,
        "the changelog dialog's title is the shared pane-title role relative to \
         the wordmark's display role: it measures {got:.3} where \
         {TYPE_PANE_TITLE}/{TYPE_WORDMARK} is {want:.3} (title {title:?}, \
         wordmark {wordmark:?})"
    );
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/welcome.rs"),
    )
    .expect("read ui/welcome.rs");
    const TYPE_CALLS: [&str; 4] = ["FontId::new(", ".size(", "chrome_font(", "data_font("];
    let offenders: Vec<String> = source
        .lines()
        .enumerate()
        .filter_map(|(n, line)| {
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

const TYPE_BODY_HINT: f32 = turbogit_ui::theme::TYPE_BODY;

#[track_caller]
fn clone_is_disabled(harness: &Harness<'_, AppState>) -> bool {
    harness.get_by_label("Clone").accesskit_node().is_disabled()
}

#[track_caller]
fn clone_accent_fills(harness: &Harness<'_, AppState>) -> Vec<egui::Rect> {
    let rect = harness.get_by_label("Clone").rect();
    filled_rects(harness)
        .into_iter()
        .filter(|(r, fill)| *fill == turbogit_ui::theme::Palette::ACCENT && rect.contains_rect(*r))
        .map(|(r, _)| r)
        .collect()
}

#[test]
fn clone_card_checks_the_environment_and_gates_the_control_on_it() {
    let mut fx = bare_fixture();
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "Git was not found");
    assert!(
        clone_is_disabled(&fx.harness),
        "an empty URL must not offer a clone"
    );

    type_into(
        &mut fx.harness,
        "Repository URL",
        "https://example.com/some/repo.git",
    );
    settle(&mut fx.harness);
    assert_not_painted(&fx.harness, "Git was not found");
    assert!(
        !clone_is_disabled(&fx.harness),
        "a resolvable git and a URL must leave Clone live"
    );
    assert!(
        !clone_accent_fills(&fx.harness).is_empty(),
        "a live primary Clone paints its brand fill"
    );

    fx.harness.state_mut().settings.git_executable = "/nonexistent/turbogit-missing-git".into();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "Git was not found");
    assert!(
        clone_is_disabled(&fx.harness),
        "the environment check is the second, independent gate"
    );
    assert!(
        clone_accent_fills(&fx.harness).is_empty(),
        "a gated Clone paints no brand fill; two controls gated on one check \
         would leave this live"
    );
}

#[test]
fn initialize_refuses_a_directory_already_under_git() {
    let project = tempfile::tempdir().expect("temp project dir");
    let outer = seed_repo(project.path(), "outer");
    let nested = outer.join("inner");
    std::fs::create_dir_all(&nested).expect("create nested dir");

    let picked = nested.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);
    fx.harness.get_by_label("Initialize Repository").click();
    settle(&mut fx.harness);

    assert_painted(&fx.harness, "already under Git");
    assert!(
        !nested.join(".git").exists(),
        "a refused init must not create a repository"
    );
    assert!(
        fx.harness.state().show_welcome(),
        "the guard leaves the user on Welcome rather than entering the directory"
    );

    let clean = project.path().join("fresh");
    std::fs::create_dir_all(&clean).expect("create init target");
    let picked = clean.clone();
    let mut fx = fixture_with_picker(move || Some(picked.clone()));
    settle(&mut fx.harness);
    fx.harness.get_by_label("Initialize Repository").click();
    settle(&mut fx.harness);

    assert_not_painted(&fx.harness, "already under Git");
    assert!(
        clean.join(".git").exists(),
        "the Initialize card must still create a real repository"
    );
    assert!(
        !fx.harness.state().show_welcome(),
        "a successful init still enters the shell"
    );
}
