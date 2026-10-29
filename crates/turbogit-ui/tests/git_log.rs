use std::path::{Path, PathBuf};

use egui::{Color32, Key, Modifiers, Pos2, Rect, Shape};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use tempfile::TempDir;
use test_support::git_seed::{commit, git};
use test_support::harness::{
    assert_menu_item_gated, assert_not_painted, assert_painted, click_menu_item, filled_circles,
    filled_rects, galley_origin, painted_text, right_click_row, settle, stroked_rects,
};
use test_support::srcscan::{self, code_only, code_with_literals};
use test_support::wcag::contrast;
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Dialog, Tab};
use turbogit_domain::model::{LogOpts, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{Palette, configure_style, install_fonts};

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
    c1: String,
    c2: String,
    c3: String,
}

fn commit_file(dir: &Path, name: &str, msg: &str) -> String {
    commit(dir, name, msg, msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    std::fs::create_dir_all(&beta).expect("beta dir");

    git(&alpha, &["init", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    let c1 = commit_file(&alpha, "file.txt", "alpha: initial commit");
    git(&alpha, &["tag", "v1.0"]);

    let remote = tmp.path().join("origin.git");
    git(
        &alpha,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &alpha,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&alpha, &["push", "-u", "origin", "main"]);

    git(&alpha, &["checkout", "-b", "feature"]);
    let _cf = commit_file(&alpha, "feature.txt", "alpha: feature work");
    git(&alpha, &["checkout", "main"]);

    std::fs::write(alpha.join("file.txt"), "alpha: second commit\n").expect("rewrite");
    git(&alpha, &["add", "."]);
    git(
        &alpha,
        &[
            "commit",
            "-m",
            "alpha: second commit",
            "-m",
            "body line for details view",
        ],
    );
    let c2 = git(&alpha, &["rev-parse", "HEAD"]).trim().to_owned();

    std::fs::create_dir_all(alpha.join("src/main")).expect("src dir");
    std::fs::write(alpha.join("src/main/app.rs"), "fn main() {}\n").expect("app file");
    std::fs::write(
        alpha.join("logo.png"),
        [0x89, b'P', b'N', b'G', 0x00, 0xFF, 0xFE],
    )
    .expect("logo file");
    let c3 = commit_file(&alpha, "README.md", "alpha: docs commit");

    git(&beta, &["init", "-b", "main"]);
    git(&beta, &["config", "user.email", "test@example.com"]);
    git(&beta, &["config", "user.name", "Test"]);
    let _b1 = commit_file(&beta, "beta.txt", "beta: root commit");

    Seed {
        _tmp: tmp,
        project,
        alpha,
        beta,
        c1,
        c2,
        c3,
    }
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 2, "both roots discovered");
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    harness_with(state)
}

fn warm_log_and_refs(state: &mut AppState) {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
        let deco = engine.ref_decorations(&root.path).expect("decorations");
        state
            .tx
            .send(AppEvent::RefsLoaded {
                root: root.id.clone(),
                deco: Ok(deco),
            })
            .expect("send RefsLoaded");
    }
    state.drain_events();
}

const LOG_BOX: egui::Vec2 =
    egui::Vec2::new(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0);

fn harness_sized(state: AppState, size: egui::Vec2) -> Harness<'static, AppState> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(size);
    settle(&mut harness);
    harness
}

fn harness_with(state: AppState) -> Harness<'static, AppState> {
    harness_sized(state, LOG_BOX)
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

#[test]
fn a_narrow_and_short_window_still_yield_without_swallowing_the_graph() {
    let seed = seeded_project();
    let mut state = AppState::new(seed.project.clone());
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    state.ui.selected_commit = Some(seed.c2.clone());
    let harness = harness_sized(state, egui::vec2(560.0, 380.0));

    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "COMMIT DETAILS");
    assert_painted(&harness, "HASH");
    assert_painted(&harness, "DATE");

    let graph_head = galley_origin(&harness, "HASH").expect("graph column header paints");
    let files = galley_origin(&harness, "CHANGED FILES").expect("files header");
    let details = galley_origin(&harness, "COMMIT DETAILS").expect("details header");

    assert!(
        graph_head.x < files.x,
        "the graph band must sit left of the right column: header at {}, column at {}",
        graph_head.x,
        files.x
    );
    assert!(
        details.y - files.y >= 24.0,
        "the details pane must yield height to the changed-files pane, got a \
         {}px gap between their headers",
        details.y - files.y
    );
}

fn count_exact(harness: &Harness<'_, AppState>, text: &str) -> usize {
    harness
        .output()
        .shapes
        .iter()
        .filter(|cl| matches!(&cl.shape, Shape::Text(s) if s.galley.text() == text))
        .count()
}

fn label_pills(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    let graph = graph_region(harness);
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| {
            *c == Palette::SURFACE_3
                && r.height() >= 14.0
                && r.height() <= 22.0
                && r.width() >= 20.0
                && r.width() <= 28.0
                && r.intersects(graph)
        })
        .map(|(r, _)| r)
        .collect()
}

#[test]
fn four_panes_render_in_mockup_layout_with_token_styling() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    assert_painted(&harness, "BRANCHES");
    assert_painted(&harness, "COMMITS");
    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "COMMIT DETAILS");
    assert_painted(&harness, "ROOTS");

    assert_painted(&harness, "Search branches");
    assert_painted(&harness, "Search commits");

    let branches = branches_region(&harness);
    assert!(
        (branches.left() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 24.0,
        "branches pane must hug the left edge of the log body (right of \
         the workspace sidebar): left={:?}",
        branches.left()
    );

    let files = files_region(&harness);
    let details = details_region(&harness);
    let body_right = filled_rects(&harness)
        .iter()
        .map(|(r, _)| r.right())
        .fold(f32::NEG_INFINITY, f32::max);
    for (name, card) in [("changed files", files), ("details", details)] {
        assert!(
            (334.0..=354.0).contains(&card.width()) && (body_right - card.right()).abs() <= 12.0,
            "the {name} card must be the right column's ~344px width and reach the \
             body's right edge: {card:?}, body right {body_right}"
        );
    }

    assert!(
        (430.0..=450.0).contains(&details.height()),
        "details pane must keep D4's ~440px card, got {:.0}px",
        details.height()
    );
    assert!(
        details.bottom() >= files.bottom() - 8.0 && files.top() < details.top(),
        "the details card is pinned below the changed-files card, got \
         files {files:?} and details {details:?}"
    );
}

#[test]
fn ref_labels_collapse_into_a_single_pill_revealed_on_hover() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let root = RootId(seed.alpha.clone().into());
    let mut commits = harness.state().caches.log(&root).unwrap().to_vec();
    let newest = commits.iter().map(|commit| commit.time).max().unwrap();
    commits
        .iter_mut()
        .find(|commit| commit.id == seed.c1)
        .unwrap()
        .time = newest + 86_400;
    harness.state_mut().caches.store_log(root, commits);
    settle(&mut harness);

    assert_eq!(
        label_pills(&harness).len(),
        3,
        "one label pill per decorated commit; the ref-less row gets none"
    );

    for pill in label_pills(&harness) {
        assert!(
            pill.width() < 120.0 && pill.height() <= 22.0,
            "label pills are compact pills, not full rows"
        );
    }

    let graph_text = painted_in_region(&harness, graph_region(&harness));
    for name in ["main", "origin/main", "v1.0", "feature"] {
        assert!(
            !graph_text.iter().any(|t| t == name),
            "`{name}` must not be painted inline in the graph (only on hover)"
        );
    }

    let hash = galley_origin(&harness, &short(&seed.c1)).expect("c1 hash is painted");
    let pills = label_pills(&harness);
    let matching: Vec<_> = pills
        .iter()
        .filter(|pill| pill.y_range().contains(hash.y))
        .collect();
    assert_eq!(matching.len(), 1, "exactly one pill on c1's row");
    let pill = matching[0];
    assert!(
        pills.iter().any(|other| other.top() > pill.top()),
        "fixture must exercise c1 away from the bottom pill"
    );
    assert!(
        graph_region(&harness).contains(pill.center()),
        "hover point must be inside the visible graph"
    );
    let before_remote = count_exact(&harness, "origin/main");
    let before_tag = count_exact(&harness, "v1.0");
    let before = before_remote + before_tag;
    harness.hover_at(pill.center());
    settle(&mut harness);
    let after = count_exact(&harness, "origin/main") + count_exact(&harness, "v1.0");
    assert!(
        after > before,
        "hovering the labels pill must reveal the ref names (before={before}, after={after})"
    );
    assert_eq!(
        count_exact(&harness, "origin/main"),
        before_remote + 1,
        "c1 tooltip reveals its remote"
    );
    assert_eq!(
        count_exact(&harness, "v1.0"),
        before_tag + 1,
        "c1 tooltip reveals its tag"
    );
}

#[test]
fn log_renders_empty_first_and_decorations_appear_once_refs_loaded_lands() {
    let seed = seeded_project();
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 2, "both roots discovered");
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    state.ui.tab = Tab::Log;
    let mut harness = harness_with(state);
    let alpha_id = RootId(seed.alpha.clone().into());

    assert!(
        !harness.state().caches.refs_loaded(&alpha_id),
        "refs must not be loaded on the render thread; the data-ensure step \
         only kicks the worker"
    );
    assert_eq!(
        label_pills(&harness).len(),
        0,
        "no decoration pills before RefsLoaded lands"
    );
    assert_painted(&harness, "alpha: second commit");

    let deco = engine.ref_decorations(&seed.alpha).expect("decorations");
    harness
        .state_mut()
        .tx
        .send(AppEvent::RefsLoaded {
            root: alpha_id.clone(),
            deco: Ok(deco),
        })
        .expect("send RefsLoaded");
    harness.state_mut().drain_events();
    settle(&mut harness);

    assert!(
        harness.state().caches.refs_loaded(&alpha_id),
        "the drained RefsLoaded must fill the ref cache"
    );
    assert!(
        !label_pills(&harness).is_empty(),
        "decoration pills must appear after RefsLoaded lands"
    );
}

#[test]
fn root_stripes_appear_and_roots_filter_narrows_displayed_commits() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "beta: root commit");
    assert!(
        harness
            .state()
            .multi
            .roots
            .iter()
            .any(|r| r.id == RootId(seed.beta.clone().into())),
        "both roots must be registered"
    );

    let mut stripe_colors: Vec<Color32> = filled_rects(&harness)
        .into_iter()
        .filter(|(r, _)| r.width() <= 5.0 && r.height() >= 15.0)
        .map(|(_, c)| c)
        .collect();
    stripe_colors.sort_by_key(|c| c.r() as u32 * 1_000_000 + c.g() as u32 * 1_000 + c.b() as u32);
    stripe_colors.dedup();
    assert!(
        stripe_colors.len() >= 2,
        "expected distinct root stripes for two roots, got {stripe_colors:?}"
    );

    harness.get_by_label("Root alpha").click();
    settle(&mut harness);

    assert_not_painted(&harness, "beta: root commit");
    assert_painted(&harness, "alpha: second commit");
    assert_eq!(
        harness.state().ui.log_root_filter,
        Some(RootId(seed.alpha.clone().into())),
        "roots filter state must track the selection"
    );

    harness.get_by_label("All roots").click();
    settle(&mut harness);
    assert_painted(&harness, "beta: root commit");
    assert_eq!(harness.state().ui.log_root_filter, None);
}

#[test]
fn details_pane_shows_hash_author_date_parents_message_for_selection() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c2.as_str()),
        "clicking a log row must select the commit"
    );

    assert_painted(&harness, &short(&seed.c2));
    assert_not_painted(&harness, "click to copy full hash");
    assert_painted(&harness, "Test");
    assert_painted(&harness, "test@example.com");
    assert_painted(&harness, "Date");
    assert_painted(&harness, "Parents");
    assert_painted(&harness, &short(&seed.c1));

    assert_painted(&harness, "body line for details view");
    let hash_pos = galley_origin(&harness, &short(&seed.c2)).expect("hash painted");
    let body_pos = galley_origin(&harness, "body line for details view").expect("message painted");
    assert!(
        body_pos.y > hash_pos.y,
        "full message must render below the key-value block"
    );
}

#[test]
fn selection_uses_translucent_highlight_not_solid_brand() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    let positions: Vec<Pos2> = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text().contains("alpha: second commit") => {
                Some(shape.pos)
            }
            _ => None,
        })
        .collect();
    assert!(!positions.is_empty(), "selected subject must be painted");
    let sel_rects: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, c)| *c == Palette::selection_bg())
        .map(|(r, _)| r)
        .collect();
    assert!(
        !sel_rects.is_empty(),
        "selection must paint the translucent `selection_bg()` focus band"
    );
    assert!(
        positions
            .iter()
            .any(|p| sel_rects.iter().any(|r| r.contains(*p))),
        "translucent selection must cover the selected row"
    );

    let win_w = 1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH;
    let solid_brand_rows: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::BRAND && r.width() > 200.0 && r.height() >= 20.0)
        .filter(|(r, _)| r.center().x < win_w * 0.7)
        .map(|(r, _)| r)
        .collect();
    assert!(
        solid_brand_rows.is_empty(),
        "graph selection must stay translucent, got {solid_brand_rows:?}"
    );
}

fn painted_in_region(harness: &Harness<'_, AppState>, region: Rect) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if region.contains(shape.pos) => {
                Some(shape.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

fn branches_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 200.0 && r.width() <= 220.0)
        .map(|(r, _)| r)
        .expect("branches pane card not painted")
}

fn graph_region(harness: &Harness<'_, AppState>) -> Rect {
    let branches = branches_region(harness);
    let body_right = filled_rects(harness)
        .iter()
        .map(|(r, _)| r.right())
        .fold(f32::NEG_INFINITY, f32::max);
    Rect::from_min_max(
        Pos2::new(branches.right(), branches.top()),
        Pos2::new(body_right - 330.0, branches.bottom()),
    )
}

#[test]
fn branches_search_filters_live_as_text_is_typed() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let region = branches_region(&harness);
    let in_pane = |harness: &Harness<'_, AppState>| painted_in_region(harness, region);
    assert!(
        in_pane(&harness).iter().any(|t| t == "feature"),
        "LOCAL feature listed initially"
    );
    assert!(
        in_pane(&harness).iter().any(|t| t == "main"),
        "LOCAL main listed initially"
    );

    harness.get_by_label("Search branches").click();
    harness.get_by_label("Search branches").type_text("feature");
    settle(&mut harness);

    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "main")
            .is_none(),
        "unmatched branch rows must disappear while typing"
    );
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Button, "v1.0")
            .is_none(),
        "unmatched tag rows must disappear while typing"
    );
    let texts = in_pane(&harness);
    assert!(
        texts.iter().any(|t| t.contains("feature")),
        "matching branch must remain: {texts:?}"
    );
}

#[test]
fn graph_search_filters_live_as_text_is_typed() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    assert_painted(&harness, "alpha: initial commit");
    assert_painted(&harness, "beta: root commit");

    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text("second");
    settle(&mut harness);

    assert_painted(&harness, "alpha: second commit");
    assert_not_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "beta: root commit");
}

#[test]
fn changed_files_pane_lists_selected_commit_files_with_status_badges() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    assert_painted(&harness, "CHANGED FILES");

    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "file.txt");

    let pos = galley_origin(&harness, "M").expect("status badge painted");
    let expected = turbogit_ui::ui::widgets::BadgeKind::Modified.colors().bg;
    let badge = filled_rects(&harness)
        .into_iter()
        .find(|(r, c)| *c == expected && r.contains(pos))
        .expect("modified badge pill not painted with its token tint");
    assert!(badge.0.width() < 40.0, "badges are compact pills");

    let galley = harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == "M" => Some(shape.galley.clone()),
            _ => None,
        })
        .expect("status badge galley");
    assert_eq!(badge.0.height(), turbogit_ui::ui::widgets::CHIP_HEIGHT);
    assert_eq!(
        pos.x - badge.0.left(),
        turbogit_ui::ui::widgets::CHIP_PAD_X,
        "the shared chip's text inset on the left"
    );
    assert_eq!(
        badge.0.right() - (pos.x + galley.size().x),
        turbogit_ui::ui::widgets::CHIP_PAD_X,
        "the shared chip's text inset on the right"
    );
    assert!((pos.x - (badge.0.center().x - galley.size().x / 2.0)).abs() < 0.01);
    assert!((pos.y - (badge.0.center().y - galley.size().y / 2.0)).abs() < 0.01);
    let badge_shape = harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Rect(rect) if rect.fill == expected && rect.rect.contains(pos) => {
                Some(rect.clone())
            }
            _ => None,
        })
        .expect("modified badge rect");
    assert_eq!(
        badge_shape.corner_radius,
        egui::CornerRadius::same(turbogit_ui::theme::CONTROL_RADIUS)
    );
}

fn settle_pumped(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.state_mut().drain_events();
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("log layout did not settle within 10 pumped frames");
}

fn wait_for_file_stats(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        harness.state_mut().drain_events();
        harness.step();
        let loaded = harness
            .state()
            .ui
            .selected_commit
            .as_ref()
            .is_some_and(|cid| {
                harness
                    .state()
                    .selected_root
                    .as_ref()
                    .is_some_and(|root| harness.state().caches.file_stats_loaded(root, cid))
            });
        if loaded {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the selected commit's file stats never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn select_second_commit(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    wait_for_file_stats(harness);
    settle_pumped(harness);
}

fn select_docs_commit(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: docs commit", short(&seed.c3));
    harness.get_by_label(&row_label).click();
    wait_for_file_stats(harness);
    settle_pumped(harness);
}

#[test]
fn the_files_pane_names_its_three_empty_states_apart() {
    let seed = seeded_project();
    git(
        &seed.alpha,
        &["commit", "--allow-empty", "-q", "-m", "nothing changed"],
    );
    let mut harness = log_harness(&seed);

    assert_painted(&harness, "Select a commit to see its changed files.");

    select_docs_commit(&mut harness, &seed);
    harness.get_by_label("Filter changed files").click();
    harness
        .get_by_label("Filter changed files")
        .type_text("zzz");
    settle_pumped(&mut harness);
    assert_painted(&harness, "No file matches the filter.");
    assert_not_painted(&harness, "No changed files.");

    let head = head_of(&seed);
    harness
        .get_by_label(&format!("{} nothing changed", short(&head)))
        .click();
    settle_pumped(&mut harness);
    assert_painted(&harness, "No changed files.");
    assert_not_painted(&harness, "No file matches the filter.");
}

fn head_of(seed: &Seed) -> String {
    git(&seed.alpha, &["rev-parse", "HEAD"]).trim().to_string()
}

fn details_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 334.0 && r.width() <= 354.0)
        .map(|(r, _)| r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("details pane card not painted")
}

fn files_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 334.0 && r.width() <= 354.0)
        .map(|(r, _)| r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("changed-files pane card not painted")
}

fn commits_region(harness: &Harness<'_, AppState>) -> Rect {
    let branches = branches_region(harness);
    let files = files_region(harness);
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::CONTENT_BG && r.height() > 100.0)
        .filter(|(r, _)| r.left() >= branches.right() && r.right() <= files.left())
        .map(|(r, _)| r)
        .max_by_key(|r| r.width().round() as i64)
        .expect("commits pane card not painted between the branches card and the right column")
}

/// The width of the run of consecutive one-point columns of `x_range` that this
/// frame paints as **app background and nothing else**, read off one horizontal
/// scan line at `y`, together with every `(rect, fill)` that interrupted it.
///
/// **Why the scan line and not `commits.left() - branches.right()`.** The gap
/// between two card rects is arithmetic about the layout; the gutter is a claim
/// about the *frame*, and the two are not the same claim. Arithmetic would be
/// satisfied by three cards that merely touch with nothing behind them, and it
/// would be satisfied just as happily by a fourth surface painted straight
/// across the gap, "the cards do not overlap" is not "the cards are separated".
/// So this walks the painted columns and asks, of each one, what is actually
/// drawn there. A column counts only if every fill covering it is
/// [`Palette::BG`], which is the app background the central panel paints behind
/// the log body, and coverage is half-open (`min < x + 1 && x < max`) so that a
/// card *ending* at the column's edge is not reported as covering it: the
/// question is what is inside the gutter, not where its neighbour stops. A
/// fully transparent rect is not a fill and is skipped, exactly as the rest of
/// this suite's rect probes skip it, egui emits one per un-framed panel.
///
/// Text and circles are deliberately not consulted. They are the panes' content,
/// they live inside the cards, and a glyph straddling a gutter would be a
/// different defect; the rule this ratchets is about **surfaces**, and a surface
/// is a filled rect. The interruptions are returned rather than swallowed so a
/// failure names the surface that got there.
fn background_run(
    harness: &Harness<'_, AppState>,
    y: f32,
    x_range: std::ops::RangeInclusive<f32>,
) -> (f32, Vec<(Rect, Color32)>) {
    let shapes = &harness.output().shapes;
    let mut run = 0.0f32;
    let mut best: f32 = 0.0;
    let mut interruptions: Vec<(Rect, Color32)> = Vec::new();
    let mut x = *x_range.start();
    while x < *x_range.end() {
        let over: Vec<(Rect, Color32)> = shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(r)
                    if r.fill != Color32::TRANSPARENT
                        && r.fill != Palette::BG
                        && r.rect.min.x < x + 1.0
                        && x < r.rect.max.x
                        && r.rect.min.y <= y
                        && y < r.rect.max.y =>
                {
                    Some((r.rect, r.fill))
                }
                _ => None,
            })
            .collect();
        if over.is_empty() {
            run += 1.0;
            best = best.max(run);
        } else {
            run = 0.0;
            interruptions.extend(over);
        }
        x += 1.0;
    }
    interruptions.sort_by_key(|(rect, _)| (rect.left().round() as i64, rect.top().round() as i64));
    interruptions.dedup();
    (best, interruptions)
}

fn crosses_card(stroked: Rect, card: Rect) -> bool {
    const EPS: f32 = 0.5;
    stroked.intersects(card)
        && card.contains(stroked.center())
        && !card.shrink(EPS).contains_rect(stroked)
}

fn vertical_lines(harness: &Harness<'_, AppState>) -> Vec<(f32, (f32, f32))> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::LineSegment { points, .. } if points[0].x == points[1].x => {
                Some((points[0].x, (points[0].y, points[1].y)))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_details_pane_leads_with_the_subject_and_drops_the_kv_wall() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let painted = painted_in_region(&harness, details_region(&harness));
    assert!(
        painted.iter().any(|t| t == "COMMIT DETAILS"),
        "the pane keeps its header: {painted:?}"
    );
    assert!(
        painted.iter().any(|t| t == "alpha: second commit"),
        "the subject is the pane's identity: {painted:?}"
    );
    for gone in ["Hash:", "Author:"] {
        assert!(
            !painted.iter().any(|t| t.starts_with(gone)),
            "`{gone}` belonged to the kv wall the redesign replaces: {painted:?}"
        );
    }
}

#[test]
fn the_details_pane_states_the_author_once_as_a_key_value_row() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let painted = painted_in_region(&harness, details_region(&harness));
    assert!(
        painted.iter().any(|t| t == "Author"),
        "the author is a key in the fact list: {painted:?}"
    );
    assert!(
        painted.iter().any(|t| t == "Test <test@example.com>"),
        "name and email together as the value: {painted:?}"
    );
    assert!(
        !painted.iter().any(|t| t == "T"),
        "the avatar card is gone: initials alone were the card's own first \
         rendering of a fact the key/value row now states once. {painted:?}"
    );
    assert!(
        !painted.iter().any(|t| t == "test@example.com"),
        "…and the email is no longer a second, unlabelled rendering of the same \
         value: {painted:?}"
    );
    assert_eq!(
        painted.iter().filter(|t| t.starts_with("Test <")).count(),
        2,
        "the `Name <email>` value appears exactly twice — the author's and the \
         committer's, which are the same person in this fixture and a genuinely \
         different one in a rebased repository. {painted:?}"
    );
}

#[test]
fn the_churn_summary_replaces_the_status_count_string() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let painted = painted_in_region(&harness, details_region(&harness));
    assert!(
        painted.iter().any(|t| t == "1 file changed"),
        "the file count leads the summary: {painted:?}"
    );
    assert!(painted.iter().any(|t| t == "+1"), "totals: {painted:?}");
    assert!(painted.iter().any(|t| t == "−1"), "totals: {painted:?}");
    assert!(
        !painted.iter().any(|t| t == "1 modified"),
        "the old status-count string is gone: {painted:?}"
    );
    let region = details_region(&harness);
    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(r, c)| *c == Palette::STATE_SUCCESS && region.contains(r.center())),
        "the churn bar paints its added segment"
    );
}

#[test]
fn a_changed_file_row_opens_the_shared_menu_and_its_verbs_still_work() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    right_click_row(&mut harness, "file.txt");

    assert!(
        stroked_rects(&harness)
            .iter()
            .any(|(_, stroke, _)| *stroke == Palette::LINE),
        "the row's menu paints inside the shared surface frame"
    );
    assert_painted(&harness, "Show blame");
    assert_painted(&harness, "Show history for file…");

    click_menu_item(&mut harness, "Show blame", "Show blame");
    settle_pumped(&mut harness);
    let blame = harness.state().ui.blame.clone().expect("blame opened");
    assert_eq!(blame.path, PathBuf::from("file.txt"));
    assert_eq!(blame.rev, seed.c2, "blamed at the commit that was selected");

    right_click_row(&mut harness, "file.txt");
    click_menu_item(&mut harness, "Show blame", "Show history for file…");
    settle_pumped(&mut harness);
    assert_eq!(
        harness.state().ui.log_path_scope,
        Some(PathBuf::from("file.txt")),
        "the row's other verb still does what it did"
    );
}

#[test]
fn a_blocked_file_row_item_stays_visible_and_states_its_reason() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("file.txt"));
    settle_pumped(&mut harness);

    right_click_row(&mut harness, "file.txt");
    assert_menu_item_gated(&mut harness, "Show blame", "Show history for file…");
    assert_stated_on_hover(
        &mut harness,
        "Show history for file…",
        "already showing this file's history",
    );
}

#[test]
fn no_surface_builds_a_context_menu_out_of_stock_egui_anymore() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let offenders: Vec<String> = srcscan::rust_files(&src)
        .into_iter()
        .filter(|path| {
            std::fs::read_to_string(path)
                .unwrap()
                .contains("response.context_menu(")
        })
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        offenders.is_empty(),
        "a stock egui context menu is still built in {offenders:?}"
    );
}

#[test]
fn every_verb_the_pane_performed_is_reachable_from_the_commit_menu() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    right_click_row(&mut harness, &row_label);

    for verb in [
        "Cherry-pick to…",
        "Cherry-pick across…",
        "Revert commit",
        "New branch",
    ] {
        assert_painted(&harness, verb);
    }

    assert_menu_item_gated(&mut harness, "Copy hash", "Revert commit");
    click_menu_item(&mut harness, "Copy hash", "Revert commit");
    settle_pumped(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "revert stays gated while the current branch is protected"
    );

    right_click_row(&mut harness, &row_label);
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle_pumped(&mut harness);
    assert!(
        matches!(harness.state().ui.dialog, Some(Dialog::NewBranch)),
        "the click must still open the new-branch dialog"
    );
}

#[test]
fn the_guardrail_reason_now_sits_on_the_item_it_blocks() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    select_second_commit(&mut harness, &seed);
    assert_not_painted(
        &harness,
        "'main' is a protected branch — reverting commits on it is not allowed",
    );

    right_click_row(&mut harness, &row_label);
    assert_stated_on_hover(
        &mut harness,
        "Revert commit",
        "the current branch is protected",
    );
}

/// Travel a real pointer into the open menu's disabled row until its reason
/// paints. A single teleport onto a disabled row never registers as hover.
#[track_caller]
fn assert_stated_on_hover(harness: &mut Harness<'_, AppState>, label: &str, reason: &str) {
    harness.remove_cursor();
    harness.step();
    let rect = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(label.to_string()))
        .unwrap_or_else(|| panic!("menu item {label}"))
        .rect();
    harness.hover_at(rect.center() - egui::vec2(0.0, 3.0));
    harness.step();
    harness.hover_at(rect.center());
    for _ in 0..80 {
        harness.step();
        if painted_text(harness).iter().any(|t| t.contains(reason)) {
            return;
        }
    }
    panic!("the blocked {label} item never stated {reason:?}");
}

#[test]
fn rows_carry_their_line_counts_and_a_binary_row_carries_none() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_docs_commit(&mut harness, &seed);

    // README.md and src/main/app.rs each add one line; logo.png is binary, so
    // git's numstat has no counts for it and the row shows none.
    assert_painted(&harness, "logo.png");
    assert_eq!(
        count_exact(&harness, "+1"),
        2,
        "both text rows carry their insertion count"
    );
    assert_eq!(count_exact(&harness, "+0"), 0, "an unmeasured row is blank");
    assert_eq!(count_exact(&harness, "−0"), 0, "an unmeasured row is blank");
}

#[test]
fn a_changed_file_row_puts_its_directory_under_its_name() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_docs_commit(&mut harness, &seed);

    let name = galley_origin(&harness, "app.rs").expect("file name on its own line");
    let dir = galley_origin(&harness, "src/main").expect("directory under the name");
    assert!(dir.y > name.y, "the directory sits below the name");
    assert!(dir.x >= name.x, "and starts no further left than it");
}

#[test]
fn the_changed_files_filter_narrows_rows_by_path() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_docs_commit(&mut harness, &seed);

    assert_painted(&harness, "README.md");
    assert_painted(&harness, "app.rs");

    harness.get_by_label("Filter changed files").click();
    harness
        .get_by_label("Filter changed files")
        .type_text("app");
    settle(&mut harness);

    assert_painted(&harness, "app.rs");
    assert_not_painted(&harness, "README.md");
    assert!(
        harness.state().ui.log_file_filter == "app",
        "the pane's filter is its own state, not the Commit tab's"
    );
}

#[test]
fn changed_files_count_is_its_own_pill_not_part_of_the_title() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    assert_painted(&harness, "CHANGED FILES");
    assert_not_painted(&harness, "CHANGED FILES (1)");
    let pos = galley_origin(&harness, "1").expect("count pill painted");
    assert!(
        filled_rects(&harness)
            .into_iter()
            .any(|(rect, _)| rect.contains(pos)),
        "the count must sit on a filled chip"
    );
}

fn scope_log_to_file(harness: &mut Harness<'_, AppState>, row_label: &str, file: &str) {
    harness.get_by_label(row_label).click();
    settle(harness);
    right_click_row(harness, file);
    click_menu_item(harness, "Show blame", "Show history for file…");
    settle(harness);
}

#[test]
fn show_history_for_file_scopes_the_log_to_touching_commits() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    assert_painted(&harness, "alpha: docs commit");
    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "beta: root commit");

    let docs_label = format!("{} alpha: docs commit", short(&seed.c3));
    scope_log_to_file(&mut harness, &docs_label, "README.md");

    assert!(
        harness.state().ui.log_path_scope.is_some(),
        "activating the action must record the path scope"
    );
    assert_painted(&harness, "alpha: docs commit");
    assert_not_painted(&harness, "alpha: second commit");
    assert_not_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "beta: root commit");
}

#[test]
fn scoped_history_keeps_graph_and_details_functional() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let label = format!("{} alpha: second commit", short(&seed.c2));
    scope_log_to_file(&mut harness, &label, "file.txt");

    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "alpha: docs commit");
    assert_not_painted(&harness, "beta: root commit");

    let scoped_row = format!("{} alpha: initial commit", short(&seed.c1));
    harness.get_by_label(&scoped_row).click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "rows inside the scope must stay selectable"
    );
    assert_painted(&harness, &short(&seed.c1));
    assert_painted(&harness, "Test");
    assert_painted(&harness, "Parents");

    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "file.txt");
}

#[test]
fn clearing_the_path_scope_restores_the_full_log() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let docs_label = format!("{} alpha: docs commit", short(&seed.c3));
    scope_log_to_file(&mut harness, &docs_label, "README.md");
    assert_not_painted(&harness, "beta: root commit");

    harness.get_by_label("Remove path filter").click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.log_path_scope,
        None,
        "clearing must drop the path scope"
    );
    assert_painted(&harness, "alpha: docs commit");
    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "beta: root commit");
}

#[test]
fn history_tab_is_gone_and_navigation_lands_only_on_valid_windows() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    assert_not_painted(&harness, "History");
    assert_not_painted(&harness, "Settings");

    // Every remaining tab is reachable and lands on its tool window.
    // ("Commit" is intentionally reached by keyboard only, its label also
    // exists on the toolbar button, and kittest rejects ambiguous queries.)
    harness.get_by_label("Log").click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.tab,
        Tab::Log,
        "Log tab must land on its tool window"
    );

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Commit);
}

#[test]
fn the_branches_pane_marks_the_current_branch_with_its_marker() {
    let seed = seeded_project();
    let harness = log_harness(&seed);
    let pane = branches_region(&harness);

    let (in_pane, elsewhere) = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Path(path)
                if path.stroke.color == egui::epaint::ColorMode::Solid(Palette::ACCENT_TEXT) =>
            {
                Some(path.visual_bounding_rect().center())
            }
            _ => None,
        })
        .fold((0, 0), |(mut a, mut b), pos| {
            if pane.contains(pos) {
                a += 1;
            } else {
                b += 1;
            }
            (a, b)
        });

    assert!(
        in_pane >= 1,
        "the Log pane's branch rows carry the current marker; painted galleys: {:?}",
        painted_text(&harness)
    );
    assert_eq!(
        elsewhere, 0,
        "the current marker belongs to the branches pane, not the rest of the Log window"
    );
}

use test_support::harness::{PaintedGalley, painted_galleys, painted_ink};
use turbogit_ui::theme::CARD_RADIUS;
use turbogit_ui::theme::TYPE_SECTION;
use turbogit_ui::ui::widgets::PANE_HEADER_HEIGHT;

const LOG_PANE_TITLES: [&str; 4] = ["BRANCHES", "COMMITS", "CHANGED FILES", "COMMIT DETAILS"];

const COLUMN_LABELS: [&str; 4] = ["HASH", "AUTHOR", "MESSAGE", "DATE"];

fn header_label(harness: &Harness<'_, AppState>, label: &str) -> PaintedGalley {
    let table = commit_table_region(harness);
    all_galleys(harness, label)
        .into_iter()
        .find(|g| table.contains(g.pos))
        .unwrap_or_else(|| {
            panic!(
                "the commit table must paint the `{label}` column header inside {table:?}; \
                 painted: {:#?}",
                painted_galleys(harness)
                    .iter()
                    .map(|g| (g.text.as_str(), g.pos, g.color))
                    .collect::<Vec<_>>()
            )
        })
}

fn structural_rules(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RULE_STRUCTURAL)
        .map(|(rect, _)| rect)
        .filter(|r| (r.height() - 1.0).abs() < 0.01)
        .collect()
}

#[track_caller]
fn the_galley(harness: &Harness<'_, AppState>, text: &str) -> PaintedGalley {
    painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == text)
        .unwrap_or_else(|| {
            panic!(
                "`{text}` was not painted; painted text:\n{:#?}",
                painted_galleys(harness)
                    .into_iter()
                    .map(|g| g.text)
                    .collect::<Vec<_>>()
            )
        })
}

fn all_galleys(harness: &Harness<'_, AppState>, text: &str) -> Vec<PaintedGalley> {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text)
        .collect()
}

/// The laid-out font size of the galley painting `text`.
///
/// Off the galley's own layout job rather than off its painted box: a galley's
/// rect height is the *line* height the font reports, which is not the type
/// size (9px type lays out a 10px line), and a ratchet that compares one to the
/// other is a ratchet about the font.
fn painted_font_size(harness: &Harness<'_, AppState>, text: &str) -> f32 {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => shape
                .galley
                .job
                .sections
                .first()
                .map(|section| section.format.font_id.size),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{text}` is not painted, so it has no font size"))
}

#[derive(Debug)]
struct PaintedHeader {
    title: String,
    offset: Pos2,
    rule: Rect,
    size: f32,
    label_height: f32,
    family: egui::FontFamily,
    ink: Color32,
}

#[track_caller]
fn measure_pane_header(harness: &Harness<'_, AppState>, title: &str) -> PaintedHeader {
    let galley = the_galley(harness, title);
    let rule = structural_rules(harness)
        .into_iter()
        .filter(|r| (r.left() - galley.pos.x).abs() < 0.01 && r.top() >= galley.pos.y)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| {
            panic!(
                "the `{title}` pane paints no `RULE_STRUCTURAL` hairline under its title, \
                 so it is not wearing the shared pane header. Rules: {:?}",
                structural_rules(harness)
            )
        });
    PaintedHeader {
        title: galley.text.clone(),
        offset: Pos2::new(galley.pos.x - rule.left(), galley.pos.y - rule.top()),
        rule,
        size: painted_font_size(harness, title),
        label_height: galley.rect.height(),
        family: galley.family.clone(),
        ink: galley.color,
    }
}

#[test]
fn every_log_pane_wears_the_one_shared_pane_header() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let measured: Vec<PaintedHeader> = LOG_PANE_TITLES
        .iter()
        .map(|title| measure_pane_header(&harness, title))
        .collect();
    assert_eq!(
        measured
            .iter()
            .map(|h| h.title.as_str())
            .collect::<Vec<_>>(),
        LOG_PANE_TITLES,
        "each named pane must have rendered its own title"
    );

    let first = &measured[0];
    for other in &measured[1..] {
        assert_eq!(
            (other.offset, other.label_height, other.rule.height()),
            (first.offset, first.label_height, first.rule.height()),
            "the `{}` pane's header is not the header the `{}` pane wears: \
             offset {:?} vs {:?}. Two headers is not two spellings of one \
             decision; it is a band the user has to re-read.",
            other.title,
            first.title,
            other.offset,
            first.offset
        );
        assert_eq!(
            (other.size, other.family.clone(), other.ink),
            (first.size, first.family.clone(), first.ink),
            "the `{}` pane's title is set in a different type than the `{}` pane's: \
             {:?}/{:?}/{:?} vs {:?}/{:?}/{:?}",
            other.title,
            first.title,
            other.size,
            other.family,
            other.ink,
            first.size,
            first.family,
            first.ink
        );
    }

    for header in &measured {
        let centred = (PANE_HEADER_HEIGHT - header.label_height) / 2.0;
        assert!(
            (header.offset.y + PANE_HEADER_HEIGHT - centred).abs() < 0.01,
            "the `{}` title is not centred in a {}-point band: it sits {} points \
             from the hairline, and a centred 9px label sits {}",
            header.title,
            PANE_HEADER_HEIGHT,
            header.offset.y,
            -centred
        );
        assert!(
            header.offset.x.abs() < 0.01,
            "the `{}` title must start where its own hairline starts, not float \
             inside the band: x offset {}",
            header.title,
            header.offset.x
        );
        assert_eq!(header.rule.height(), 1.0, "the hairline is one pixel");
    }

    assert_eq!(first.size, TYPE_SECTION);
    assert_eq!(first.family, egui::FontFamily::Proportional);
    assert_eq!(first.ink, Palette::INK_3);
    assert_ne!(first.ink, Palette::INK_4);
}

fn graph_band(harness: &Harness<'_, AppState>) -> Rect {
    measure_pane_header(harness, "COMMITS").rule
}

#[test]
fn the_commit_table_labels_its_columns_hash_author_message_date() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let labels: Vec<PaintedGalley> = COLUMN_LABELS
        .iter()
        .map(|label| the_galley(&harness, label))
        .collect();
    for (label, hits) in COLUMN_LABELS
        .iter()
        .zip(COLUMN_LABELS.iter().map(|l| all_galleys(&harness, l).len()))
    {
        assert_eq!(
            hits, 1,
            "the `{label}` column header paints {hits} times; a column is labelled once"
        );
    }

    let centre = labels[0].pos.y + labels[0].rect.height() / 2.0;
    for label in &labels {
        let own = label.pos.y + label.rect.height() / 2.0;
        assert!(
            (own - centre).abs() < 0.01,
            "every column label shares the header row's baseline; `{}` sits {} points \
             off the others'",
            label.text,
            own - centre
        );
    }
    for pair in labels.windows(2) {
        assert!(
            pair[0].pos.x < pair[1].pos.x,
            "the column headers read left to right as HASH | AUTHOR | MESSAGE | DATE; \
             `{}` starts at {} and `{}` at {}",
            pair[0].text,
            pair[0].pos.x,
            pair[1].text,
            pair[1].pos.x
        );
    }

    let graph = graph_band(&harness);
    let rule = structural_rules(&harness)
        .into_iter()
        .find(|r| (r.left() - graph.left()).abs() < 0.01 && r.top() >= labels[0].pos.y)
        .unwrap_or_else(|| {
            panic!(
                "the commit table underlines its column header with no structural rule \
                 at its own left edge; labels {labels:#?}, rules: {:?}",
                structural_rules(&harness)
            )
        });
    assert!(
        rule.top() >= labels[0].pos.y,
        "the column header's underline is below its labels, not above them: \
         rule {rule:?}, labels at {:?}",
        labels[0].rect
    );
    let first_row = the_galley(&harness, &short(&seed.c3));
    assert!(
        first_row.pos.y > rule.bottom(),
        "the first commit row is below the column header's underline: row {:?}, \
         rule {rule:?}",
        first_row.pos
    );
}

#[test]
fn the_log_column_headers_are_the_muted_ink_and_never_the_dim_one() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    for label in COLUMN_LABELS {
        assert_eq!(
            painted_ink(&harness, label),
            Some(Palette::INK_3),
            "the `{label}` column header is a label the user reads to know what a \
             bare number or timestamp means, so it takes the muted step"
        );
        assert_ne!(
            painted_ink(&harness, label),
            Some(Palette::INK_4),
            "9px is normal-size text and `INK_4` is 3.2:1. The dim step is reserved \
             for placeholders, dim path suffixes and hatches — never the only \
             rendering of something the user needs. The target frames were drawn \
             with the dim value before this was checked, which is why this exists."
        );
    }
    let label_font = the_galley(&harness, "HASH");
    let pane_title = the_galley(&harness, "COMMITS");
    assert!(
        (label_font.rect.height() - pane_title.rect.height()).abs() < 0.01,
        "a pane title and a column label are the same mark at two scales: {:?} vs {:?}",
        label_font.rect,
        pane_title.rect
    );
    assert_eq!(
        painted_ink(&harness, "COMMITS"),
        painted_ink(&harness, "HASH")
    );
    assert!(
        Palette::INK_3 != Palette::INK_4,
        "the two steps are still two steps"
    );
}

#[test]
fn the_commit_table_underlines_its_columns_once_in_the_structural_rule() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let graph = graph_band(&harness);
    let centre = the_galley(&harness, "HASH").pos.y;
    let first_row = the_galley(&harness, &short(&seed.c3));
    let band = Rect::from_min_max(
        Pos2::new(graph.left(), centre),
        Pos2::new(graph.right(), first_row.pos.y),
    );

    let underlines: Vec<Rect> = structural_rules(&harness)
        .into_iter()
        .filter(|r| r.top() > centre && r.bottom() < first_row.pos.y)
        .filter(|r| r.intersects(band))
        .collect();
    assert_eq!(
        underlines.len(),
        1,
        "the column header underlines itself once: {underlines:?}. Two rules between a \
         header and its rows is the nested-boxes problem this migration removes, and \
         it comes back quietly."
    );
    let rule = underlines[0];
    assert!(
        (rule.left() - graph.left()).abs() < 0.01 && (rule.width() - graph.width()).abs() < 0.01,
        "the underline spans the commit table's own width: rule {rule:?}, table {graph:?}"
    );

    let footer_rules: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RULE_FOOTER)
        .map(|(r, _)| r)
        .filter(|r| (r.height() - 1.0).abs() < 0.01)
        .filter(|r| r.intersects(band))
        .collect();
    assert!(
        footer_rules.is_empty(),
        "the column header underlines itself with the STRUCTURAL hairline; a footer \
         rule down there would read as a dialog boundary inside a table: {footer_rules:?}"
    );
    assert_ne!(
        Palette::RULE_STRUCTURAL,
        Palette::RULE_FOOTER,
        "if the two rules were ever merged this ratchet would stop meaning anything"
    );
}

#[test]
fn every_column_boundary_in_the_header_equals_its_boundary_in_the_rows() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let graph = graph_band(&harness);
    let hash = the_galley(&harness, &short(&seed.c3));

    let row_centre = hash.pos.y + hash.rect.height() / 2.0;
    let mut cells: Vec<PaintedGalley> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| {
            (g.pos.y + g.rect.height() / 2.0 - row_centre).abs() < 2.5
                && g.rect.right() <= graph.right() + 0.5
                && g.rect.left() >= graph.left() - 0.5
        })
        .collect();
    cells.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));

    assert_eq!(
        cells.len(),
        4,
        "the first commit row paints exactly four cells (hash, author, message, \
         date); found {cells:#?}"
    );
    assert_eq!(cells[0].text, short(&seed.c3), "leftmost cell is the hash");
    assert_eq!(
        cells[2].text, "alpha: docs commit",
        "third cell is the subject"
    );
    assert!(
        cells[1].rect.right() < cells[2].rect.left(),
        "the author cell ends before the message cell starts: {:?} vs {:?}",
        cells[1].rect,
        cells[2].rect
    );

    for (label, cell) in COLUMN_LABELS.iter().zip([&cells[0], &cells[1], &cells[2]]) {
        assert!(
            (the_galley(&harness, label).pos.x - cell.pos.x).abs() < 0.01,
            "the `{label}` header label is not over its cell: label at {}, cell at {}. \
             The header and the rows must read the same column table — a column that \
             is narrow in the header and wide in the rows is the eye re-anchoring on \
             every row.",
            the_galley(&harness, label).pos.x,
            cell.pos.x
        );
    }
    let date_label = the_galley(&harness, "DATE");
    assert!(
        (date_label.rect.right() - cells[3].rect.right()).abs() < 0.01,
        "a trailing column's label ends where its cell ends: label {:?}, cell {:?}",
        date_label.rect,
        cells[3].rect
    );
}

fn filled_rects_with_rounding(
    harness: &Harness<'_, AppState>,
) -> Vec<(Rect, Color32, egui::CornerRadius)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(shape) if shape.fill != Color32::TRANSPARENT => {
                Some((shape.rect, shape.fill, shape.corner_radius))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_four_log_panes_are_stroke_less_cards_and_the_details_raised_parent_is_gone() {
    let seed = seeded_project();
    let mut state = AppState::new(seed.project.clone());
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    state.ui.selected_commit = Some(seed.c3.clone());
    let mut harness = harness_with(state);
    settle(&mut harness);

    let cards: Vec<Rect> = filled_rects_with_rounding(&harness)
        .into_iter()
        .filter(|(_, fill, _)| *fill == Palette::CONTENT_BG)
        .filter(|(rect, _, _)| rect.height() > 100.0)
        .map(|(rect, _, _)| rect)
        .collect();
    assert_eq!(
        cards.len(),
        4,
        "exactly four log panes are cards — branches, commits, changed files, \
         commit details. Found {cards:#?}"
    );
    let branches = branches_region(&harness);
    let commits = commits_region(&harness);
    let files = files_region(&harness);
    let details = details_region(&harness);
    for (name, card) in [
        ("branches", branches),
        ("commits", commits),
        ("changed files", files),
        ("commit details", details),
    ] {
        assert!(
            cards.iter().any(|c| c == &card),
            "the {name} pane must paint a card at {card:?}; cards: {cards:#?}"
        );
    }

    for (rect, _, rounding) in filled_rects_with_rounding(&harness)
        .into_iter()
        .filter(|(rect, fill, _)| *fill == Palette::CONTENT_BG && rect.height() > 100.0)
    {
        assert_eq!(
            rounding,
            egui::CornerRadius::same(CARD_RADIUS),
            "the card at {rect:?} must round at `CARD_RADIUS`; a square corner reads as \
             a panel, which is what the card replaced"
        );
    }

    let strokes = stroked_rects(&harness);
    for card in [&branches, &commits, &files, &details] {
        let on_card: Vec<&(Rect, Color32, f32)> = strokes
            .iter()
            .filter(|(rect, _, _)| crosses_card(*rect, *card))
            .collect();
        assert!(
            on_card.is_empty(),
            "a log pane's card must paint NO stroke on its own edge; {on_card:?} crosses \
             the card at {card:?}. A 1px hairline means *this floats*, and none of these \
             four do."
        );
    }

    let lines = vertical_lines(&harness);
    for (name, gap) in [
        ("branches | commits", branches.right()..=commits.left()),
        ("commits | changed files", commits.right()..=files.left()),
    ] {
        let in_gutter: Vec<&(f32, (f32, f32))> =
            lines.iter().filter(|(x, _)| gap.contains(x)).collect();
        assert!(
            in_gutter.is_empty(),
            "the {name} gutter x={gap:?} carries a divider {in_gutter:?}. The columns are \
             separated by ten points of app background, not by a 1px rule: a stroke on a \
             content region's edge is R2's one reserved meaning (this floats), and none of \
             these four do."
        );
    }

    let raised_over_details: Vec<(Rect, Color32)> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::SURFACE)
        .filter(|(rect, _)| rect.intersects(details) && rect.height() > 100.0)
        .collect();
    assert!(
        raised_over_details.is_empty(),
        "the details pane is a card on the app background, not a raised band inside a \
         background panel: {raised_over_details:?} still paints `SURFACE` over its \
         card at {details:?}"
    );
    assert!(
        details.top() >= files.bottom(),
        "the details card sits below the changed-files card, not over it: \
         files {files:?}, details {details:?}"
    );
}

#[test]
fn the_three_log_panes_are_separated_by_ten_points_of_app_background() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let branches = branches_region(&harness);
    let commits = commits_region(&harness);
    let files = files_region(&harness);
    assert_eq!(
        commits.height(),
        branches.height().max(files.height()),
        "the commits card should be a full-height pane between the two outer columns, \
         not a card sized to its content. commits {commits:?}, branches {branches:?}, \
         files {files:?}"
    );

    const MIN_GUTTER: f32 = 8.0;
    const FRAME_GUTTER: f32 = 10.0;

    let scan_y = commits.center().y;

    for (name, from, to, left_card, right_card) in [
        (
            "branches | commits",
            branches.right(),
            commits.left(),
            branches,
            commits,
        ),
        (
            "commits | changed files",
            commits.right(),
            files.left(),
            commits,
            files,
        ),
    ] {
        let gap = to - from;
        assert!(
            gap >= MIN_GUTTER,
            "R2 pane separation: there must be at least {MIN_GUTTER}pt of app background \
             between the {name} card, and there is {gap}pt — {left_card:?} then {right_card:?}. \
             Cards that touch are one region wearing three fills, which is the nested-boxes \
             shape the card migration removed."
        );
        assert!(
            (gap - FRAME_GUTTER).abs() < 0.01,
            "R2 pane separation: the {name} gap must be the approved frame's {FRAME_GUTTER}pt \
             of air, and it is {gap}pt — {left_card:?} then {right_card:?}. A gap that is \
             merely non-zero has found its width somewhere other than the frame."
        );

        let (run, interruptions) = background_run(&harness, scan_y, from..=to);
        assert!(
            run >= MIN_GUTTER,
            "R2 pane separation: the {name} gap must be app background. Scanning the frame \
             at y={scan_y} from x={from} to x={to} found only {run}pt of `Palette::BG`, and \
             the rest is covered by {interruptions:?}. The gap has to be air, not a card \
             fill and not a fourth surface painted across it."
        );
        assert!(
            run >= FRAME_GUTTER,
            "R2 pane separation: scanning y={scan_y} across the {name} gap measures {run}pt \
             of `Palette::BG` between x={from} and x={to} (covered by {interruptions:?}), \
             short of the frame's {FRAME_GUTTER}pt. The cards are separated, but not by the \
             width the design drew."
        );
    }
}

const LOG_SRC: &str = include_str!("../src/ui/log_window.rs");

const COLOUR_LITERALS: [&str; 8] = [
    "Color32::from_rgb",
    "Color32::from_rgba",
    "Color32::from_rgba_unmultiplied",
    "Color32::from_gray",
    "Color32::from_black",
    "Color32::from_white",
    "Color32::from_hex",
    "0x",
];

#[test]
fn the_logs_chrome_names_no_ink_outside_the_shared_ramp() {
    for (name, src) in [
        ("micro_text", srcscan::fn_text(LOG_SRC, "micro_text")),
        ("header_cells", srcscan::fn_text(LOG_SRC, "header_cells")),
    ] {
        for literal in COLOUR_LITERALS {
            assert!(
                !src.contains(literal),
                "`fn {name}` names a colour literal (`{literal}`) instead of a token. \
                 The log's chrome takes its ink from the shared ramp, so a local literal \
                 here is a colour that answers to nothing and drifts the moment the token \
                 moves. `fn {name}`:\n{src}"
            );
        }
    }

    let seed = seeded_project();
    let mut state = AppState::new(seed.project.clone());
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    state.ui.selected_commit = Some(seed.c2.clone());
    let mut harness = harness_with(state);
    settle(&mut harness);

    let details = details_region(&harness);
    for label in ["Author", "Date", "Committer", "Parents", "1 file changed"] {
        let painted = all_galleys(&harness, label)
            .into_iter()
            .find(|g| details.contains(g.pos))
            .unwrap_or_else(|| {
                panic!(
                    "the details pane must paint `{label}`; it painted {:?}",
                    painted_text(&harness)
                )
            });
        assert_eq!(
            painted.color,
            Palette::INK_3,
            "the details pane's `{label}` is metadata, and metadata is the muted step"
        );
        assert_ne!(
            painted.color,
            Palette::INK_4,
            "`{label}` may not be the dim step: it is one of the only renderings of a \
             commit's own facts, and `INK_4` is 3.2:1"
        );
    }
}

fn commit_row_label(id: &str, subject: &str) -> String {
    format!("{} {subject}", short(id))
}

fn commit_table_region(harness: &Harness<'_, AppState>) -> Rect {
    let card = commits_region(harness);
    Rect::from_x_y_ranges(graph_band(harness).x_range(), card.y_range())
}

fn selection_band_over(host: Color32) -> Color32 {
    let alpha = f32::from(Palette::selection_bg().a()) / 255.0;
    let blend = |premultiplied: u8, base: u8| {
        (f32::from(premultiplied) + f32::from(base) * (1.0 - alpha)).round() as u8
    };
    let band = Palette::selection_bg();
    Color32::from_rgb(
        blend(band.r(), host.r()),
        blend(band.g(), host.g()),
        blend(band.b(), host.b()),
    )
}

fn fills_in(harness: &Harness<'_, AppState>, region: Rect, color: Color32) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, painted)| *painted == color && region.intersect(*rect) == *rect)
        .map(|(rect, _)| rect)
        .collect()
}

#[test]
fn a_chosen_commit_row_paints_the_row_selected_fill_and_one_brand_rail() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let label = commit_row_label(&seed.c2, "alpha: second commit");
    let row = harness.get_by_label(&label).rect();
    let table = commit_table_region(&harness);

    let bands = fills_in(&harness, table, Palette::selection_bg());
    assert_eq!(
        bands,
        vec![row],
        "exactly one chosen row, painting the row-fill decision's focus band at \
         its own rect; painted {bands:?} for row {row:?}"
    );
    assert_eq!(
        row.height(),
        turbogit_ui::theme::FILE_ROW_HEIGHT,
        "the band is the row's own height: a shell that measured the row would \
         paint a different one"
    );

    let host_assertion = filled_rects(&harness)
        .into_iter()
        .any(|(rect, color)| color == Palette::BG && rect.contains_rect(row));
    assert!(
        host_assertion,
        "the commit table sits on the app background (the graph pane is the panel \
         itself), so the band composites over `Palette::BG`"
    );
    let composited = selection_band_over(Palette::BG);
    for (channel, got, want) in [
        ("r", composited.r(), Palette::ROW_SELECTED.r()),
        ("g", composited.g(), Palette::ROW_SELECTED.g()),
        ("b", composited.b(), Palette::ROW_SELECTED.b()),
    ] {
        assert!(
            (i32::from(got) - i32::from(want)).abs() <= 1,
            "the chosen row's band composites to the selected-list-row fill: \
             {channel} {got} vs {want} (composited {composited:?} vs \
             ROW_SELECTED {:?})",
            Palette::ROW_SELECTED
        );
    }

    let rails = filled_rects(&harness)
        .into_iter()
        .filter(|(rect, color)| {
            *color == Palette::BRAND
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && table.intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .collect::<Vec<_>>();
    assert_eq!(
        rails.len(),
        1,
        "one chosen row carries exactly one rail: {rails:?}"
    );
    let rail = rails[0];
    assert_eq!(
        rail.left(),
        row.left(),
        "the rail is at the row's leading edge"
    );
    assert_eq!(
        rail.width(),
        turbogit_ui::theme::RAIL_WIDTH,
        "the rail is `theme::RAIL_WIDTH` wide, read from the token by the one rail \
         painter rather than restated at this site"
    );
    assert_eq!(
        rail.height(),
        row.height(),
        "the rail spans the row's full height: {rail:?} vs row {row:?}"
    );
    assert_eq!(rail.top(), row.top());
    assert_eq!(rail.right(), row.left() + turbogit_ui::theme::RAIL_WIDTH);

    let plain_label = commit_row_label(&seed.c3, "alpha: docs commit");
    let plain = harness.get_by_label(&plain_label).rect();
    assert!(
        !plain.contains_rect(rail),
        "the unchosen row at {plain:?} must not carry the chosen row's rail {rail:?}"
    );
    assert!(
        fills_in(&harness, table, Palette::selection_bg())
            .iter()
            .all(|band| *band != plain),
        "and it must not carry the band either: exactly one chosen row"
    );
}

#[test]
fn no_commit_row_fills_with_the_current_ref_or_the_accent_token() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);
    let table = commit_table_region(&harness);

    let selection_fills = fills_in(&harness, table, Palette::SELECTION);
    assert!(
        selection_fills.is_empty(),
        "the current-ref band is not a list-row fill: {selection_fills:?} paint \
         `SELECTION` inside the commit table"
    );

    let accent_fills = filled_rects(&harness)
        .into_iter()
        .filter(|(rect, color)| *color == Palette::BRAND && table.intersect(*rect) == *rect)
        .map(|(rect, _)| rect)
        .collect::<Vec<_>>();
    for rect in &accent_fills {
        assert!(
            (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01,
            "the only accent fill a commit row may paint is its rail, which is \
             `RAIL_WIDTH` wide at the leading edge; {rect:?} is {accent_fills:?}"
        );
    }
    assert_eq!(
        accent_fills.len(),
        1,
        "and there is one of them, for the one chosen row: {accent_fills:?}"
    );
}

struct Columns {
    hash: f32,
    author: f32,
    message: f32,
    date_right: f32,
}

fn commit_columns(harness: &Harness<'_, AppState>) -> Columns {
    let header = |needle: &str| {
        all_galleys(harness, needle)
            .into_iter()
            .find(|g| commit_table_region(harness).contains(g.pos))
            .unwrap_or_else(|| panic!("the column header `{needle}` paints in the table"))
    };
    Columns {
        hash: header("HASH").pos.x,
        author: header("AUTHOR").pos.x,
        message: header("MESSAGE").pos.x,
        date_right: header("DATE").rect.right(),
    }
}

fn cell(
    harness: &Harness<'_, AppState>,
    row: Rect,
    columns: &Columns,
    column: &str,
) -> test_support::harness::PaintedGalley {
    let matches = |g: &test_support::harness::PaintedGalley| match column {
        "HASH" | "AUTHOR" | "MESSAGE" => (g.pos.x - anchor(columns, column)).abs() < 2.0,
        "DATE" => (g.rect.right() - columns.date_right).abs() < 2.0,
        other => panic!("no cell named {other}"),
    };
    let hits: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| row.contains(g.pos) && matches(g))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {column} cell inside {row:?}, found {hits:#?}"
    );
    hits.into_iter().next().expect("one hit")
}

fn anchor(columns: &Columns, column: &str) -> f32 {
    match column {
        "HASH" => columns.hash,
        "AUTHOR" => columns.author,
        "MESSAGE" => columns.message,
        other => panic!("{other} is not a left-anchored column"),
    }
}

#[test]
fn a_chosen_commit_row_paints_its_text_in_the_same_ink_as_an_unchosen_row() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let chosen = harness
        .get_by_label(&commit_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    let plain = harness
        .get_by_label(&commit_row_label(&seed.c3, "alpha: docs commit"))
        .rect();
    let columns = commit_columns(&harness);

    let resting = Palette::BG;
    let chosen_band = selection_band_over(resting);

    for (column, expected) in [
        ("HASH", Palette::LINK),
        ("AUTHOR", Palette::INK_2),
        ("MESSAGE", Palette::INK),
        ("DATE", Palette::INK_2),
    ] {
        let on_chosen = cell(&harness, chosen, &columns, column);
        let on_plain = cell(&harness, plain, &columns, column);
        println!(
            "{column}: chosen {:?} on {chosen_band:?}, plain {:?} on {resting:?}",
            on_chosen.color, on_plain.color
        );
        assert_eq!(
            on_chosen.color, on_plain.color,
            "{column}: selection must not recolour a row's text — the same cell on a \
             chosen row and an unchosen row paints one ink"
        );
        assert_eq!(
            on_chosen.color, expected,
            "{column}: the row's ink is a named step of the shared ramp, not whatever \
             it happened to inherit"
        );
        for (what, surface) in [
            ("its resting surface", resting),
            ("its chosen band", chosen_band),
        ] {
            let ratio = contrast(on_chosen.color, surface);
            assert!(
                ratio >= 4.5,
                "{column}: {:?} must clear AA on {what} ({surface:?}): {ratio:.2}:1",
                on_chosen.color
            );
        }
        assert_ne!(
            on_chosen.color,
            Palette::INK_3,
            "{column}: the muted step is 3.76:1 on the band a chosen row takes"
        );
    }

    assert!(
        contrast(Palette::BRAND, chosen_band) < 4.5,
        "if the accent ever clears AA on the chosen band, this test's hash \
         expectation should be revisited rather than left stale"
    );
    assert!(
        contrast(Palette::LINK, chosen_band) >= 4.5,
        "the hash's `LINK` does clear it, which is why the hash moved"
    );
}

#[test]
fn a_chosen_commit_row_starts_every_column_where_an_unchosen_row_does() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let chosen = harness
        .get_by_label(&commit_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    let plain = harness
        .get_by_label(&commit_row_label(&seed.c3, "alpha: docs commit"))
        .rect();
    let columns = commit_columns(&harness);

    assert_eq!(
        chosen.size(),
        plain.size(),
        "a chosen row is the same size as an unchosen one: reserving the rail's \
         width, or painting it only when chosen, resizes the row"
    );
    for column in ["HASH", "AUTHOR", "MESSAGE", "DATE"] {
        let on_chosen = cell(&harness, chosen, &columns, column);
        let on_plain = cell(&harness, plain, &columns, column);
        if column == "DATE" {
            assert!(
                (on_chosen.rect.right() - on_plain.rect.right()).abs() < 0.01,
                "the right-aligned date column ends at the same x chosen or not: \
                 {} vs {}",
                on_chosen.rect.right(),
                on_plain.rect.right()
            );
        } else {
            assert!(
                (on_chosen.pos.x - on_plain.pos.x).abs() < 0.01,
                "the {column} column starts at the same x chosen or not: {} vs {}",
                on_chosen.pos.x,
                on_plain.pos.x
            );
        }
    }

    let bands = fills_in(
        &harness,
        commit_table_region(&harness),
        Palette::selection_bg(),
    );
    assert_eq!(
        bands,
        vec![chosen],
        "the band starts at the row's left edge"
    );
    assert_eq!(bands[0].left(), plain.left());
}

#[test]
fn the_root_stripe_shares_the_leading_gutter_with_the_rail() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);
    let table = commit_table_region(&harness);

    let chosen = harness
        .get_by_label(&commit_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    let plain = harness
        .get_by_label(&commit_row_label(&seed.c3, "alpha: docs commit"))
        .rect();

    let stripe_of = |row: Rect| -> (Rect, Color32) {
        let candidates: Vec<(Rect, Color32)> = filled_rects(&harness)
            .into_iter()
            .filter(|(rect, _)| {
                (rect.width() - 3.0).abs() < 0.01
                    && row.contains_rect(*rect)
                    && table.intersect(*rect) == *rect
            })
            .collect();
        assert_eq!(
            candidates.len(),
            1,
            "one full-height 3px stripe per row; found {candidates:?} inside {row:?}"
        );
        candidates.into_iter().next().expect("one stripe")
    };
    let (chosen_stripe, chosen_color) = stripe_of(chosen);
    let (plain_stripe, _) = stripe_of(plain);

    assert_eq!(
        chosen_stripe.height(),
        chosen.height(),
        "the stripe spans the row's full height"
    );
    assert_eq!(
        chosen_stripe.left(),
        plain_stripe.left(),
        "the stripe sits in the same place whether or not the row is chosen — a \
         marker that moved with selection would move the row's content with it"
    );
    assert_eq!(
        chosen_stripe.height(),
        plain_stripe.height(),
        "and it is the same height, so choosing a row resizes nothing"
    );

    let rail = filled_rects(&harness)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::BRAND
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && table.intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .expect("the chosen row's rail");
    assert_eq!(
        rail.left(),
        chosen.left(),
        "the rail leads at the row's leading edge"
    );
    assert_eq!(
        chosen_stripe.left(),
        rail.right(),
        "the stripe sits immediately inside the rail, so both are on screen: the \
         decision is to share the gutter, not to let one displace the other"
    );
    assert_eq!(
        rail.right() - chosen.left() + chosen_stripe.width(),
        turbogit_ui::theme::RAIL_WIDTH + 3.0,
        "the gutter is the rail plus the stripe and nothing more: the cells sit \
         where they sat before the rail existed"
    );

    let columns = commit_columns(&harness);
    for row in [chosen, plain] {
        for column in ["HASH", "AUTHOR", "MESSAGE"] {
            let painted = cell(&harness, row, &columns, column);
            assert!(
                painted.pos.x > chosen_stripe.right(),
                "the {column} column clears the gutter in every row: {} is not right \
                 of the stripe's trailing edge {}",
                painted.pos.x,
                chosen_stripe.right()
            );
        }
    }

    let node = filled_circles(&harness)
        .into_iter()
        .find(|(center, _, _)| chosen.contains(*center))
        .map(|(_, _, color)| color);
    assert_eq!(
        node,
        Some(chosen_color),
        "the lane node and the stripe are the same root's colour"
    );
}

#[test]
fn no_row_in_the_log_window_indents_when_it_is_chosen() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let branches = branches_region(&harness);
    let roots_x = |harness: &Harness<'_, AppState>, label: &str| -> f32 {
        all_galleys(harness, label)
            .into_iter()
            .find(|g| branches.contains(g.pos))
            .unwrap_or_else(|| panic!("the ROOTS filter paints `{label}`"))
            .pos
            .x
    };
    let all_roots_at_rest = roots_x(&harness, "All roots");
    let root_alpha_at_rest = roots_x(&harness, "Root alpha");

    harness.get_by_label("Root alpha").click();
    settle(&mut harness);
    assert_eq!(
        roots_x(&harness, "All roots"),
        all_roots_at_rest,
        "losing the selection must not indent the row's label: the rail is paint, \
         not padding"
    );
    assert_eq!(
        roots_x(&harness, "Root alpha"),
        root_alpha_at_rest,
        "and gaining it must not either"
    );

    let files = files_region(&harness);
    let name_in = |harness: &Harness<'_, AppState>, path: &str| -> f32 {
        all_galleys(harness, path)
            .into_iter()
            .find(|g| files.contains(g.pos))
            .unwrap_or_else(|| panic!("the changed-files pane paints `{path}`"))
            .pos
            .x
    };
    let first_file = harness.state().ui.log_selected_file.clone();
    assert!(first_file.is_none(), "no file is chosen to begin with");
    let path = {
        let listed = harness
            .state()
            .caches
            .files_for(
                harness
                    .state()
                    .selected_root
                    .as_ref()
                    .expect("a root is selected"),
                &seed.c2,
            )
            .map(|files| files.first().map(|c| c.path.display().to_string()))
            .unwrap_or_default();
        listed.unwrap_or_else(|| panic!("the chosen commit lists a changed file"))
    };
    let name_at_rest = name_in(&harness, &path);
    harness.get_by_label(&path).click();
    settle(&mut harness);
    assert_eq!(
        harness
            .state()
            .ui
            .log_selected_file
            .as_ref()
            .map(|p| p.display().to_string()),
        Some(path.clone()),
        "clicking a file row chooses it, so this measures a chosen row"
    );
    assert_eq!(
        name_in(&harness, &path),
        name_at_rest,
        "a chosen file row's name starts where an unchosen one's does"
    );
}

#[test]
fn the_collapsed_ref_marker_is_the_neutral_badge_and_not_a_ref_chip() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    settle(&mut harness);

    let graph = commit_table_region(&harness);
    let markers: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(rect, color)| {
            *color == turbogit_ui::ui::widgets::BadgeKind::Neutral.colors().bg
                && (rect.height() - turbogit_ui::ui::widgets::CHIP_HEIGHT).abs() < 0.01
                && graph.intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        !markers.is_empty(),
        "a decorated commit paints its collapsed ref marker"
    );

    let rounding_of = |color: Color32, height: f32, region: Rect| -> Vec<u8> {
        harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(rect_shape)
                    if rect_shape.fill == color
                        && (rect_shape.rect.height() - height).abs() < 0.01
                        && region.intersect(rect_shape.rect) == rect_shape.rect =>
                {
                    Some(rect_shape.corner_radius.nw)
                }
                _ => None,
            })
            .collect()
    };
    let marker_radii = rounding_of(
        turbogit_ui::ui::widgets::BadgeKind::Neutral.colors().bg,
        turbogit_ui::ui::widgets::CHIP_HEIGHT,
        graph,
    );
    for radius in &marker_radii {
        assert_eq!(
            *radius,
            turbogit_ui::theme::PILL_RADIUS,
            "the log's collapsed marker wears the **pill** radius — the badge's \
             slot, not the chips': {marker_radii:?}"
        );
        assert_ne!(
            *radius,
            turbogit_ui::theme::CHIP_RADIUS,
            "a marker that carries no ref name must not wear the chip radius: the \
             chip radius means `a ref name`, and spending it here would make it \
             mean two things"
        );
    }
    let chip_radii = rounding_of(
        turbogit_ui::ui::widgets::REF_CHIP_COLORS.bg,
        turbogit_ui::ui::widgets::CHIP_HEIGHT,
        branches_region(&harness),
    );
    assert!(
        chip_radii.contains(&(turbogit_ui::theme::CHIP_RADIUS)),
        "the branches pane paints real ref chips at the chip radius in the same \
         frame: {chip_radii:?}"
    );

    for chip_value in [
        "REF_CHIP_COLORS",
        "COMPACT_CHIP_GEOMETRY",
        "ref_chip",
        "compact_chip_radius",
    ] {
        assert!(
            !code_only(LOG_SRC).contains(chip_value),
            "the log names `{chip_value}`: a painter-level sibling of the ref chip \
             would be a second function producing a ref chip, outside the module \
             whose ratchet says there is exactly one. The collapsed marker is the \
             neutral badge — see `paint_label_pill`."
        );
    }
}

#[test]
fn the_log_rows_take_their_ink_from_the_shared_ramp() {
    assert!(
        !LOG_SRC.contains("fn row_ink("),
        "the log's local `row_ink(active)` is gone: it inverted ink with selection, \
         which is the one thing the shared row grammar exists to stop. `row_name_ink` \
         and `row_meta_ink` are the log's whole row-ink vocabulary now, and both are \
         selection-independent."
    );
    for (name, source) in [
        ("row_name_ink", srcscan::fn_text(LOG_SRC, "row_name_ink")),
        ("row_meta_ink", srcscan::fn_text(LOG_SRC, "row_meta_ink")),
        ("commit_row", srcscan::fn_text(LOG_SRC, "commit_row")),
        ("file_row", srcscan::fn_text(LOG_SRC, "file_row")),
        ("paint_log_row", srcscan::fn_text(LOG_SRC, "paint_log_row")),
    ] {
        for literal in COLOUR_LITERALS {
            assert!(
                !source.contains(literal),
                "`fn {name}` names a colour literal (`{literal}`) instead of a token: \
                 a row's ink is the shared ramp's, and a local literal is a colour \
                 that answers to nothing. `fn {name}`:\n{source}"
            );
        }
    }

    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);
    let chosen = harness
        .get_by_label(&commit_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    let plain = harness
        .get_by_label(&commit_row_label(&seed.c3, "alpha: docs commit"))
        .rect();

    assert_eq!(
        cell(&harness, chosen, &commit_columns(&harness), "MESSAGE").color,
        Palette::INK,
        "a row's own name reads at primary — the same ink whether the row is chosen \
         or not, which is what `row_name_ink` is for"
    );
    assert_eq!(
        cell(&harness, plain, &commit_columns(&harness), "MESSAGE").color,
        Palette::INK,
        "…on an unchosen row too: selection does not lighten or darken the name"
    );

    for label in ["All roots", "Root alpha"] {
        let painted = all_galleys(&harness, label)
            .into_iter()
            .find(|g| branches_region(&harness).contains(g.pos))
            .unwrap_or_else(|| panic!("the ROOTS filter paints `{label}`"));
        assert_eq!(
            painted.color,
            Palette::INK,
            "`{label}` is a row's identity text, so it reads at primary whether or \
             not the filter is on it — the local inversion used to darken it"
        );
    }
}

#[test]
fn the_roots_column_is_a_labelled_column_of_the_commit_table() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let table = commit_table_region(&harness);
    let roots = header_label(&harness, "ROOTS");
    let labels: Vec<PaintedGalley> = ["HASH", "AUTHOR", "MESSAGE", "DATE"]
        .iter()
        .map(|label| header_label(&harness, label))
        .collect();

    let centre = roots.pos.y + roots.rect.height() / 2.0;
    for label in &labels {
        let own = label.pos.y + label.rect.height() / 2.0;
        assert!(
            (own - centre).abs() < 0.01,
            "the ROOTS label shares the commit table's header row baseline with \
             `{}`; it is {} points off it",
            label.text,
            own - centre
        );
    }
    let mut reading = vec![roots.pos.x];
    reading.extend(labels.iter().map(|l| l.pos.x));
    for pair in reading.windows(2) {
        assert!(
            pair[0] < pair[1],
            "the columns read left to right as ROOTS | HASH | AUTHOR | MESSAGE | \
             DATE; {} starts at {} and the next at {}",
            pair[0],
            pair[0],
            pair[1]
        );
    }

    assert_eq!(
        roots.color,
        Palette::INK_3,
        "a column label is something the user reads to know what a colour means; \
         the muted step is the column chrome's ink"
    );
    assert_ne!(
        roots.color,
        Palette::INK_4,
        "9px is normal-size text and `INK_4` is 3.2:1 — never the only rendering \
         of a label the user needs"
    );
    assert!(
        (roots.rect.height() - labels[0].rect.height()).abs() < 0.01,
        "the ROOTS label is laid out in the same type as the other column \
         labels: {:?} vs {:?}",
        roots.rect,
        labels[0].rect
    );

    let row = harness
        .get_by_label(&commit_row_label(&seed.c3, "alpha: docs commit"))
        .rect();
    assert!(
        (roots.pos.x - row.left()).abs() < 0.01,
        "the ROOTS label paints at the row's leading edge, which is the column \
         it names: label at {}, row at {row:?}",
        roots.pos.x
    );
    let swatch = roots_swatch(&harness, row).expect("the row paints its ROOTS cell");
    assert!(
        roots.pos.x <= swatch.0.left() && swatch.0.right() <= table.left() + 8.0,
        "the column is the gutter, so its cell is beside its label: label at {}, \
         swatch {:?}, table {table:?}",
        roots.pos.x,
        swatch.0
    );
    assert!(
        roots.rect.right() <= labels[0].pos.x,
        "the overhang may not reach the next label: ROOTS ends at {}, HASH starts \
         at {}. A 9px word does not fit a 5px column; this is what bounds it.",
        roots.rect.right(),
        labels[0].pos.x
    );

    let with_strings = code_with_literals(LOG_SRC);
    assert!(
        with_strings.contains(r#"PaneColumn::start("ROOTS""#),
        "the ROOTS label must be an entry of the commit table's own column \
         table, not a label painted free-floating above the gutter"
    );
    let header = srcscan::fn_text(LOG_SRC, "header_cells");
    assert!(
        header.contains("COMMIT_COLUMNS") && header.contains("column_header"),
        "the header row must still be the shared `widgets::column_header` reading \
        `COMMIT_COLUMNS`:\n{header}"
    );
    assert_eq!(
        all_galleys(&harness, "ROOTS")
            .iter()
            .filter(|g| commit_table_region(&harness).contains(g.pos))
            .count(),
        1,
        "exactly one ROOTS label inside the commit table — a second is a second \
         header. The other one in this frame is the branches pane's ROOTS *filter* \
         group title, which is a different surface and legitimately shares the word."
    );
}

fn roots_swatch(harness: &Harness<'_, AppState>, row: Rect) -> Option<(Rect, Color32)> {
    let table = commit_table_region(harness);
    let candidates: Vec<(Rect, Color32)> = filled_rects(harness)
        .into_iter()
        .filter(|(rect, _)| {
            (rect.width() - 3.0).abs() < 0.01
                && row.contains_rect(*rect)
                && table.intersect(*rect) == *rect
        })
        .collect();
    (candidates.len() == 1).then(|| candidates[0])
}

#[test]
fn the_roots_column_the_lane_and_the_legend_paint_one_colour_per_root() {
    let code = code_with_literals(LOG_SRC);
    let table_at = code
        .find("const GRAPH_COLORS")
        .expect("the one root-colour table is in the log window");
    let table_end = code[table_at..]
        .find("];")
        .expect("GRAPH_COLORS is a bracketed list")
        + table_at;
    let (inside, outside) = code.split_at(table_end);
    for literal in COLOUR_LITERALS {
        assert!(
            !outside.contains(literal),
            "the log names a colour literal (`{literal}`) outside `GRAPH_COLORS`, \
             which is the single root-colour table. A second literal is a second \
             palette, and a root's colour would then differ between the ROOTS \
             column, the graph lane and the legend:\n{}",
            &outside[..outside.len().min(2000)]
        );
    }
    assert_eq!(
        inside.matches("Color32::from_rgb").count(),
        8,
        "the table is the whole palette — eight entries, so `root_color` wraps a \
         project's roots rather than running off the end"
    );
    for (name, source) in [
        (
            "paint_root_swatch",
            srcscan::fn_text_code(LOG_SRC, "paint_root_swatch"),
        ),
        ("commit_row", srcscan::fn_text_code(LOG_SRC, "commit_row")),
        ("graph_pane", srcscan::fn_text_code(LOG_SRC, "graph_pane")),
        (
            "roots_filter_section",
            srcscan::fn_text_code(LOG_SRC, "roots_filter_section"),
        ),
    ] {
        assert!(
            source.contains("root_color("),
            "`fn {name}` paints a per-root marker, so it must ask `root_color` for \
             the colour rather than carrying one:\n{source}"
        );
    }

    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    settle(&mut harness);
    let table = commit_table_region(&harness);
    let header_top = header_label(&harness, "HASH").pos.y;

    let mut legend: Vec<(Rect, Color32)> = filled_rects(&harness)
        .into_iter()
        .filter(|(rect, _)| {
            (rect.width() - 10.0).abs() < 0.01
                && (rect.height() - 10.0).abs() < 0.01
                && rect.top() < header_top
                && table.intersect(*rect) == *rect
        })
        .collect();
    legend.sort_by(|a, b| a.0.left().total_cmp(&b.0.left()));
    let legend: Vec<Color32> = legend.into_iter().map(|(_, color)| color).collect();
    let root_count = harness.state().multi.roots.len();
    assert_eq!(
        legend.len(),
        root_count,
        "the legend paints one swatch per root, in root order: {legend:?}"
    );

    let mut seen: Vec<Color32> = Vec::new();
    for (index, legend_color) in legend.iter().enumerate() {
        let root = harness.state().multi.roots[index].id.clone();
        let commit = harness
            .state()
            .caches
            .log(&root)
            .and_then(|commits| commits.first())
            .unwrap_or_else(|| panic!("root {index} has a loaded history"))
            .clone();
        let row_label = format!(
            "{} {}",
            short(&commit.id),
            commit.message.lines().next().unwrap_or_default()
        );
        let row = harness.get_by_label(&row_label).rect();

        let (cell, cell_color) = roots_swatch(&harness, row)
            .unwrap_or_else(|| panic!("the ROOTS column paints a cell for root {index}"));
        let lane_color = filled_circles(&harness)
            .into_iter()
            .find(|(center, _, _)| row.contains(*center))
            .map(|(_, _, color)| color);
        assert_eq!(
            lane_color,
            Some(cell_color),
            "root {index}: the graph lane node and the ROOTS column paint the same \
             root's colour — lane {lane_color:?}, column {cell_color:?} at {cell:?}"
        );
        assert_eq!(
            *legend_color, cell_color,
            "root {index}: the legend swatch and the ROOTS column paint the same \
             root's colour — legend {legend_color:?}, column {cell_color:?}"
        );
        assert_ne!(
            cell_color,
            Palette::BG,
            "the gutter swatch is a root colour, not the pane's surface"
        );
        seen.push(cell_color);
    }
    assert_eq!(
        seen.iter().collect::<std::collections::HashSet<_>>().len(),
        root_count,
        "each root gets its own colour out of the one table: {seen:?}"
    );
}

#[test]
fn the_roots_column_does_not_move_the_rail() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let chosen = harness
        .get_by_label(&commit_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    let (swatch, _) = roots_swatch(&harness, chosen).expect("the chosen row keeps its swatch");
    let rail = filled_rects(&harness)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::BRAND
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && commit_table_region(&harness).intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .expect("the chosen row's rail");
    assert_eq!(
        rail.left(),
        chosen.left(),
        "the rail leads flush at the row's leading edge with the ROOTS column \
         present — the column is inside the gutter the rail already led, so it \
         never displaces it: rail {rail:?}, row {chosen:?}"
    );
    assert_eq!(
        swatch.left(),
        rail.right(),
        "and the swatch sits immediately inside it: both are on screen at once"
    );
    assert_eq!(
        rail.right() - chosen.left() + swatch.width(),
        5.0,
        "the gutter is the rail plus the swatch and nothing more"
    );

    let mut scoped = log_harness(&seed);
    select_second_commit(&mut scoped, &seed);
    scoped.state_mut().ui.log_path_scope = Some(std::path::PathBuf::from("file.txt"));
    settle(&mut scoped);
    assert!(
        scoped.state().ui.log_path_scope.is_some(),
        "the scope is applied, which is what removes the multi-root union"
    );
    let scoped_row = scoped
        .get_by_label(&commit_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    assert!(
        roots_swatch(&scoped, scoped_row).is_none(),
        "the premise: a path-scoped listing is single-root, so the ROOTS column \
         paints no cell at all"
    );
    let scoped_rail = filled_rects(&scoped)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::BRAND
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && commit_table_region(&scoped).intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .expect("the chosen row's rail in the scoped listing");
    assert_eq!(
        scoped_rail.left(),
        scoped_row.left(),
        "with the ROOTS column gone the rail still leads flush — it is the \
         column that moved nothing, not the rail that moved"
    );
    assert_eq!(
        (scoped_rail.left(), scoped_rail.width()),
        (rail.left(), rail.width()),
        "…and it is at the same absolute x and the same width in both listings, so \
         the column is genuinely free: the message column is where it was before \
         the column existed"
    );
    let columns = commit_columns(&harness);
    let hash_in_multi = cell(&harness, chosen, &columns, "HASH").pos.x;
    let scoped_columns = commit_columns(&scoped);
    let hash_scoped = cell(&scoped, scoped_row, &scoped_columns, "HASH").pos.x;
    assert!(
        hash_in_multi > chosen.right() - chosen.width(),
        "the HASH cell clears the gutter in the multi-root listing: {} is right of \
         the swatch's trailing edge {}",
        hash_in_multi,
        swatch.right()
    );
    assert!(
        (hash_in_multi - chosen.left()) > (hash_scoped - scoped_row.left()),
        "the multi-root listing pays the 5px gutter in cell position and the \
         scoped one does not, which is the whole of the column's width cost"
    );
}

struct DetailFact {
    key: PaintedGalley,
    value: PaintedGalley,
}

fn detail_fact(harness: &Harness<'_, AppState>, key: &str) -> DetailFact {
    let details = details_region(harness);
    let key_galley = all_galleys(harness, key)
        .into_iter()
        .find(|g| details.contains(g.pos))
        .unwrap_or_else(|| {
            panic!(
                "the details pane must paint the `{key}` key inside {details:?}; \
                 it painted {:?}",
                painted_in_region(harness, details)
            )
        });
    let key_mid = key_galley.pos.y + key_galley.rect.height() / 2.0;
    let value = painted_galleys(harness)
        .into_iter()
        .filter(|g| {
            g.pos.x > key_galley.rect.right()
                && (g.pos.y + g.rect.height() / 2.0 - key_mid).abs() < 2.5
                && details.contains(g.pos)
        })
        .min_by(|a, b| a.pos.x.total_cmp(&b.pos.x))
        .unwrap_or_else(|| {
            panic!(
                "the `{key}` key has no value to its right on its own line — a \
                 key with nothing beside it is not a key/value row. Painted in \
                 the pane: {:?}",
                painted_in_region(harness, details)
            )
        });
    DetailFact {
        key: key_galley,
        value,
    }
}

#[test]
fn the_details_pane_lists_author_date_and_parents_above_the_message_body() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let details = details_region(&harness);
    let facts: Vec<DetailFact> = ["Author", "Date", "Committer", "Parents"]
        .iter()
        .map(|key| detail_fact(&harness, key))
        .collect();

    assert_eq!(facts.len(), 4, "four key/value rows");

    for fact in &facts {
        assert!(
            (fact.key.pos.x - facts[0].key.pos.x).abs() < 0.01,
            "every key shares one column so the values line up: `{}` starts at {}, \
             `{}` at {}",
            fact.key.text,
            fact.key.pos.x,
            facts[0].key.text,
            facts[0].key.pos.x
        );
        assert!(
            fact.value.pos.x > fact.key.rect.right(),
            "the value sits to the right of its own key: `{}` key {:?}, value at {}",
            fact.key.text,
            fact.key.rect,
            fact.value.pos.x
        );
        assert_eq!(
            fact.key.color,
            Palette::INK_3,
            "`{}` is a field label — the details pane's **key ink** is the muted \
             step, the same one the pane's micro text has always worn",
            fact.key.text
        );
        assert_ne!(
            fact.key.color,
            Palette::INK_4,
            "`{}` may not be the dim step: it is one of the only renderings of a \
             commit's own fact, and `INK_4` is 3.2:1",
            fact.key.text
        );
        assert_eq!(
            fact.value.color,
            Palette::INK,
            "`{}`'s value is the thing the key names, so it reads at the **value \
             ink**",
            fact.key.text
        );
        for (what, ink) in [("key", fact.key.color), ("value", fact.value.color)] {
            let ratio = contrast(ink, Palette::CONTENT_BG);
            assert!(
                ratio >= 4.5,
                "the {what} of `{}` must clear AA on the details card: {ink:?} is \
                 {ratio:.2}:1 on `CONTENT_BG`",
                fact.key.text
            );
        }
    }

    let parents = facts.last().expect("the parents row");
    assert_eq!(
        parents.value.text,
        short(&seed.c1),
        "the parents row's value is the parent commit's short reference"
    );

    let body = all_galleys(&harness, "body line for details view")
        .into_iter()
        .find(|g| details.contains(g.pos))
        .expect("this commit has a message body below its subject");
    for fact in &facts {
        assert!(
            fact.key.pos.y < body.pos.y,
            "the `{}` key is above the message body: key at {}, body at {}",
            fact.key.text,
            fact.key.pos.y,
            body.pos.y
        );
        assert!(
            fact.value.pos.y < body.pos.y,
            "…and so is its value: value at {}, body at {}",
            fact.value.pos.y,
            body.pos.y
        );
    }
    for pair in facts.windows(2) {
        assert!(
            pair[0].key.pos.y < pair[1].key.pos.y,
            "the fact list reads top to bottom; `{}` is at {} and `{}` at {}",
            pair[0].key.text,
            pair[0].key.pos.y,
            pair[1].key.text,
            pair[1].key.pos.y
        );
    }
}

#[test]
fn the_details_pane_populates_on_selection_and_still_renders_its_empty_state() {
    let seed = seeded_project();

    let mut empty = AppState::new(seed.project.clone());
    warm_log_and_refs(&mut empty);
    empty.ui.tab = Tab::Log;
    empty.ui.selected_commit = None;
    let mut harness = harness_with(empty);
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit,
        None,
        "the premise: nothing is selected"
    );
    let details = details_region(&harness);
    assert!(
        painted_in_region(&harness, details)
            .iter()
            .any(|t| t == "Select a commit…"),
        "with nothing selected the details pane says so"
    );
    for key in ["Author", "Date", "Committer", "Parents"] {
        assert!(
            !painted_in_region(&harness, details)
                .iter()
                .any(|t| t == key),
            "the empty state paints no `{key}` row: a key with no commit behind it \
             is a lie about what is selected"
        );
    }

    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c2.as_str()),
        "clicking a commit row selects it"
    );
    let painted = painted_in_region(&harness, details_region(&harness));
    assert!(
        !painted.iter().any(|t| t == "Select a commit…"),
        "the empty state is gone once a commit is chosen: {painted:?}"
    );
    for key in ["Author", "Date", "Committer", "Parents"] {
        assert!(
            painted.iter().any(|t| t == key),
            "selecting a commit populates the `{key}` row: {painted:?}"
        );
    }
}

#[test]
fn the_details_pane_fact_list_is_its_own_type_and_not_the_menus_action_gates() {
    let code = code_with_literals(LOG_SRC);
    assert!(
        code.contains("struct DetailRow"),
        "the details pane's list is a type of its own"
    );
    let builder = srcscan::fn_text_code(LOG_SRC, "commit_detail_rows");
    assert!(
        builder.contains("&Commit"),
        "the list is built over the commit the log already holds:\n{builder}"
    );
    for forbidden in ["CommitFacts", "dirty", "protected_branch", "repo_name"] {
        assert!(
            !builder.contains(forbidden),
            "`commit_detail_rows` names `{forbidden}`: the details pane describes \
             a commit, and the commit menu's gates are a different question. The \
             similar names are exactly why this is a new type:\n{builder}"
        );
    }
    assert_eq!(
        code.matches("CommitFacts").count(),
        3,
        "`CommitFacts` appears in the log only where the menu is built and typed: \
         the import, the context menu's own call, and the commit-facts builder. A \
         fourth would be the details pane borrowing it."
    );
    assert!(
        code.contains("fn commit_facts") && code.contains("-> CommitFacts<"),
        "the menu's own gate builder still returns the menu's own type"
    );
    assert!(
        srcscan::fn_text_code(LOG_SRC, "details_pane").contains("commit_detail_rows"),
        "the details pane reads its rows from the new builder, not from the menu's \
         gates:\n{}",
        srcscan::fn_text_code(LOG_SRC, "details_pane")
    );
}
