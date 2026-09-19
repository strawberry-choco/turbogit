//! Issue #12 — Git Log four-pane workspace: branches, graph, changed files,
//! commit details.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] end-to-end
//! over a seeded multi-root project (two real git repos in a tempdir):
//!
//! - `alpha` — `main` with a tagged, remote-decorated history plus a
//!   `feature` branch (branches, a remote and a tag sprinkled over history)
//! - `beta` — a second root for the multi-root stripes / roots-filter cases
//!
//! Assertions use only public surfaces: painted output (text galleys +
//! filled rects with token colors) and public `AppState` transitions.

use std::path::{Path, PathBuf};

use egui::{Color32, Key, Modifiers, Pos2, Rect, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use tempfile::TempDir;
use turbogit_app::events::AppEvent;
use turbogit_app::state::{AppState, Dialog, Tab};
use turbogit_domain::model::{LogOpts, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{Palette, configure_style, install_fonts};

// --- Locally-defined harness helpers (issue #12; mirrors tests/common) -------

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

/// Paint-time origin of the first text galley painting exactly `text`.
fn galley_origin(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
}

/// Every filled rectangle painted by the last frame as `(rect, fill)`.
fn filled_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape) if rect_shape.fill != Color32::TRANSPARENT => {
                Some((rect_shape.rect, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

/// Step frames until the painted output stabilizes.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("log layout did not settle within 10 frames");
}

// --- Seeded multi-root fixture ------------------------------------------------

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    /// First root: decorated history (branch + remote + tag on one commit).
    alpha: PathBuf,
    /// Second root for multi-root cases.
    beta: PathBuf,
    /// `alpha` HEAD~2 — carries branch `main`, remote `origin/main`, tag `v1.0`.
    c1: String,
    /// `alpha` HEAD~1 — plain commit with a parent and a multiline message.
    c2: String,
    /// `alpha` HEAD — docs-only commit touching just `README.md`, so
    /// path-scoped history has off-path commits to hide (issue #19).
    c3: String,
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn commit_file(dir: &Path, name: &str, msg: &str) -> String {
    let file = dir.join(name);
    std::fs::write(&file, msg).expect("writing work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", msg]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    std::fs::create_dir_all(&beta).expect("beta dir");

    // --- alpha: main(c2 <- c1), tag v1.0@c1, origin/main@c1, feature@cf ---
    run_git(&alpha, &["init", "-b", "main"]);
    run_git(&alpha, &["config", "user.email", "test@example.com"]);
    run_git(&alpha, &["config", "user.name", "Test"]);
    let c1 = commit_file(&alpha, "file.txt", "alpha: initial commit");
    run_git(&alpha, &["tag", "v1.0"]);

    let remote = tmp.path().join("origin.git");
    run_git(
        &alpha,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    run_git(
        &alpha,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run_git(&alpha, &["push", "-u", "origin", "main"]);

    run_git(&alpha, &["checkout", "-b", "feature"]);
    let _cf = commit_file(&alpha, "feature.txt", "alpha: feature work");
    run_git(&alpha, &["checkout", "main"]);

    // Multiline body so the details pane paints text the row never does.
    std::fs::write(alpha.join("file.txt"), "alpha: second commit\n").expect("rewrite");
    run_git(&alpha, &["add", "."]);
    let c2_out = std::process::Command::new("git")
        .args([
            "commit",
            "-m",
            "alpha: second commit",
            "-m",
            "body line for details view",
        ])
        .current_dir(&alpha)
        .output()
        .expect("second commit");
    assert!(
        c2_out.status.success(),
        "second commit failed: {}",
        String::from_utf8_lossy(&c2_out.stderr)
    );
    let c2 = run_git(&alpha, &["rev-parse", "HEAD"]).trim().to_string();

    // Docs-only HEAD commit: touches neither file.txt nor feature.txt, so a
    // path-scoped history for those files has something to hide (issue #19).
    // Two paths, one of them nested, so the redesigned row has a directory
    // line to show and the pane's filter has rows to narrow. Plus a binary
    // file, whose numstat row carries no line counts at all.
    std::fs::create_dir_all(alpha.join("src/main")).expect("src dir");
    std::fs::write(alpha.join("src/main/app.rs"), "fn main() {}\n").expect("app file");
    std::fs::write(
        alpha.join("logo.png"),
        [0x89, b'P', b'N', b'G', 0x00, 0xFF, 0xFE],
    )
    .expect("logo file");
    let c3 = commit_file(&alpha, "README.md", "alpha: docs commit");

    // --- beta: an independent second root ---
    run_git(&beta, &["init", "-b", "main"]);
    run_git(&beta, &["config", "user.email", "test@example.com"]);
    run_git(&beta, &["config", "user.name", "Test"]);
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

/// Harness rendering the full shell with the Log tool window active over the
/// seeded project. The log cache is primed through the production event path
/// (`AppEvent::LogLoaded` / `AppEvent::RefsLoaded` via `state.tx` +
/// `drain_events()`); production fills it the same way, asynchronously.
fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 2, "both roots discovered");
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    harness_with(state)
}

/// Prime every registered root's log + ref decorations through the worker
/// event path, exactly the way `AppState::fetch_log` / `AppState::fetch_refs`
/// land in production.
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

/// The full-shell harness over an arbitrary pre-built state.
fn harness_with(state: AppState) -> Harness<'static, AppState> {
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
    // 1280px of log body plus the workspace sidebar (issue #05) on
    // the left edge — the four-pane spec widths hold at this size.
    harness.set_size(egui::vec2(
        1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        800.0,
    ));
    settle(&mut harness);
    harness
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// The same harness at an explicit window size, for the narrow/short cases.
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

/// Issue #23's guard, re-checked against the widened 344px column and the
/// 400px details pane: at 560×380 every pane must still render its own
/// content, the graph must keep a band of its own, and the details pane must
/// yield so the changed-files pane above it does not collapse.
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
    // The graph still owns a band: its column headers paint, with the message
    // column squeezed to whatever is left at this width.
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

/// Number of galleys painting exactly `text` (tooltips included).
fn count_exact(harness: &Harness<'_, AppState>, text: &str) -> usize {
    harness
        .output()
        .shapes
        .iter()
        .filter(|cl| matches!(&cl.shape, Shape::Text(s) if s.galley.text() == text))
        .count()
}

/// The label pills painted in the graph: compact (~24×18px) neutral
/// SURFACE_3 pills. The tag icon inside them paints strokes, not fills, so
/// the pill's rounded background rect is the reliable signature.
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

// --- Cycle 1: four panes render in mockup layout with token styling ----------

#[test]
fn four_panes_render_in_mockup_layout_with_token_styling() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    // Pane headers (uppercase micro-headers per spec §3.3/§8.3).
    assert_painted(&harness, "BRANCHES");
    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "COMMIT DETAILS");
    assert_painted(&harness, "ROOTS");

    // Live search inputs in both panes (placeholders as accessible labels).
    assert_painted(&harness, "Search branches");
    assert_painted(&harness, "Search commits");

    // Branches pane: ~210px SURFACE band at the far left of the body.
    let branches = filled_rects(&harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::SURFACE && r.width() >= 200.0 && r.width() <= 220.0)
        .expect("branches pane band (~210px SURFACE) not painted");
    assert!(
        (branches.0.left() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 24.0,
        "branches pane must hug the left edge of the log body (right of \
         the workspace sidebar): left={:?}",
        branches.0.left()
    );

    // Right column: ~344px wide band reaching the body's right edge (decision
    // D3 widened the §8.3 320px for the two-line file rows). The metadata rail
    // that once occupied the last 260px was removed in the local-changes
    // redesign (issue 03), so the right column extends to the window edge like
    // the pre-rail layout.
    let body_right = filled_rects(&harness)
        .iter()
        .map(|(r, _)| r.right())
        .fold(f32::NEG_INFINITY, f32::max);
    let rects = filled_rects(&harness);
    let right_col = rects
        .iter()
        .find(|(r, _)| {
            r.width() >= 334.0 && r.width() <= 354.0 && (body_right - r.right()).abs() <= 12.0
        })
        .expect("right column (~344px, reaching the body's right edge) not painted");

    // Details pane: ~440px tall SURFACE band at the bottom of the right
    // column (grew from the §8.3 200px in issue 15 for the Actions section, to
    // 340px in issue 17, and to decision D4's 440px for the redesigned subject
    // / author card / meta grid / churn / actions / alert stack).
    let details = filled_rects(&harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::SURFACE && r.height() >= 430.0 && r.height() <= 450.0)
        .expect("details pane band (~440px SURFACE) not painted");
    assert!(
        details.0.bottom() >= right_col.0.bottom() - 8.0,
        "details pane must sit at the bottom of the right column"
    );
}

// --- Cycle 2: refs collapse into one label pill; names show on hover -------

#[test]
fn ref_labels_collapse_into_a_single_pill_revealed_on_hover() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // Commit order is (time descending, hash ascending), not fixture creation
    // order. Force the decorated c1 row first so this test cannot accidentally
    // rely on it being the bottom pill when fixture commits share a second.
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

    // The union shows 4 commits: c1 (remote origin/main + tag v1.0), c3
    // (main) and the beta root (main) are decorated; the ref-less c2 row is
    // not. Each decorated commit paints exactly one compact label pill —
    // the undecorated row gets none. (The `feature` branch's commit is not
    // an ancestor of main, so it only drives the branches pane.)
    assert_eq!(
        label_pills(&harness).len(),
        3,
        "one label pill per decorated commit; the ref-less row gets none"
    );

    // The pill is compact and neutral — not a per-kind ref chip.
    for pill in label_pills(&harness) {
        assert!(
            pill.width() < 120.0 && pill.height() <= 22.0,
            "label pills are compact pills, not full rows"
        );
    }

    // No ref name is painted inline in the graph anymore (names survive only
    // as plain rows inside the branches pane, or in the hover tooltip).
    let graph_text = painted_in_region(&harness, graph_region(&harness));
    for name in ["main", "origin/main", "v1.0", "feature"] {
        assert!(
            !graph_text.iter().any(|t| t == name),
            "`{name}` must not be painted inline in the graph (only on hover)"
        );
    }

    // Locate c1 by its painted hash, not by its position in the sorted union.
    // Its pill carries origin/main + v1.0 regardless of timestamps or hashes.
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

// --- Log-open perf: empty-first render; decorations arrive via RefsLoaded --------

#[test]
fn log_renders_empty_first_and_decorations_appear_once_refs_loaded_lands() {
    let seed = seeded_project();
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 2, "both roots discovered");
    // Prime ONLY the log — the ref decorations deliberately stay cold, the
    // way they are before the worker's `RefsLoaded` event lands.
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
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    state.ui.tab = Tab::Log;
    let mut harness = harness_with(state);
    let alpha_id = RootId(seed.alpha.clone().into());

    // Empty-first: the render never waited on ref decoration loading (the
    // sync `ensure_refs` path is gone), and nothing panicked on the cold
    // cache — the log itself still paints.
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

    // Decorations appear once the event lands (the view kicks fetch_refs on
    // the cold cache, and any worker event drains through the same path).
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

// --- Cycle 3: root stripes appear; roots filter narrows displayed commits -----

#[test]
fn root_stripes_appear_and_roots_filter_narrows_displayed_commits() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // All-roots mode shows both roots' commits…
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

    // …with a colored stripe per root on each row (thin vertical bars).
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

    // Narrowing to the first root hides every other root's commits.
    harness.get_by_label("Root alpha").click();
    settle(&mut harness);

    assert_not_painted(&harness, "beta: root commit");
    assert_painted(&harness, "alpha: second commit");
    assert_eq!(
        harness.state().ui.log_root_filter,
        Some(RootId(seed.alpha.clone().into())),
        "roots filter state must track the selection"
    );

    // Back to all roots restores the union.
    harness.get_by_label("All roots").click();
    settle(&mut harness);
    assert_painted(&harness, "beta: root commit");
    assert_eq!(harness.state().ui.log_root_filter, None);
}

// --- Cycle 4: details pane shows hash / author / date / parents / message -----

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

    // The redesigned blocks: a copyable hash chip, an author card, and the
    // meta grid's aligned labels.
    assert_painted(&harness, &short(&seed.c2));
    assert_painted(&harness, "click to copy full hash");
    assert_painted(&harness, "Test");
    assert_painted(&harness, "test@example.com");
    assert_painted(&harness, "Date");
    assert_painted(&harness, "Parents");
    assert_painted(&harness, &short(&seed.c1));

    // The FULL message (including the body line) is painted below the kv
    // block — the graph row only ever shows the subject line.
    assert_painted(&harness, "body line for details view");
    let hash_pos = galley_origin(&harness, &short(&seed.c2)).expect("hash painted");
    let body_pos = galley_origin(&harness, "body line for details view").expect("message painted");
    assert!(
        body_pos.y > hash_pos.y,
        "full message must render below the key-value block"
    );
}

// --- Cycle 5: translucent selection that keeps lane colors readable -----------

#[test]
fn selection_uses_translucent_highlight_not_solid_brand() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    // Some galley of the selected subject sits inside a translucent
    // SELECTION_BG fill…
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
        "selection must paint the translucent SELECTION_BG token"
    );
    assert!(
        positions
            .iter()
            .any(|p| sel_rects.iter().any(|r| r.contains(*p))),
        "translucent selection must cover the selected row"
    );

    // …and nothing paints a solid BRAND row-sized highlight over the graph.
    // (The details pane's primary "Cherry-pick to…" action legitimately
    // paints a solid BRAND button — issue 15 — so the scan is scoped to the
    // graph region, the left 70% of the window; the graph itself must stay
    // translucent.)
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

// --- Cycle 6: live filtering in both search inputs -----------------------------

/// All text painted inside `region` (used to scope assertions to one pane —
/// ref names live in the branches pane, away from the graph's labels pill).
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

/// The ~210px SURFACE band of the branches pane.
fn branches_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::SURFACE && r.width() >= 200.0 && r.width() <= 220.0)
        .map(|(r, _)| r)
        .expect("branches pane band not painted")
}

/// The central graph band between the branches pane and the right column —
/// the only place ref chips ever appeared inline (now collapsed into a pill).
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

    // After the branch-tree swap (branch-tree extraction, plan step 4) the
    // pane's repo headers always paint the current branch as a chip, so
    // absence is asserted on the *row* nodes (a11y buttons), not on painted
    // text. Repo header chips paint as plain labels, never buttons.
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

// --- Cycle 7: changed-files pane lists the selected commit's files -------------

#[test]
fn changed_files_pane_lists_selected_commit_files_with_status_badges() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // Nothing selected yet → empty pane header still renders.
    assert_painted(&harness, "CHANGED FILES");

    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "file.txt");

    // Modified badge: tinted pill carrying exactly "M".
    let pos = galley_origin(&harness, "M").expect("status badge painted");
    let expected = turbogit_ui::ui::widgets::BadgeKind::Modified.colors().bg;
    let badge = filled_rects(&harness)
        .into_iter()
        .find(|(r, c)| *c == expected && r.contains(pos))
        .expect("modified badge pill not painted with its token tint");
    assert!(badge.0.width() < 40.0, "badges are compact pills");
}

// --- Redesign issue 04: the changed-files pane --------------------------------

/// Step frames until the painted output settles, pumping the worker channel
/// between them the way `src/app.rs` pumps it before rendering. The per-commit
/// file stats are a worker event, so a pane that shows them only settles this
/// way.
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

/// Pump the worker channel until the selected commit's line counts have
/// landed. The stats request is asynchronous, and a pixels-only settle can
/// stabilise in the two frames before the git call returns.
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

/// Select `alpha`'s second commit (one changed file) and settle with its
/// counts in hand.
fn select_second_commit(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    wait_for_file_stats(harness);
    settle_pumped(harness);
}

/// Select `alpha`'s docs commit (`README.md`, the nested `src/main/app.rs`, and
/// a binary `logo.png`) and settle with its counts in hand.
fn select_docs_commit(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: docs commit", short(&seed.c3));
    harness.get_by_label(&row_label).click();
    wait_for_file_stats(harness);
    settle_pumped(harness);
}

#[test]
fn the_files_pane_names_its_three_empty_states_apart() {
    let seed = seeded_project();
    // A commit that genuinely changes nothing, so "no files" has a real owner.
    run_git(
        &seed.alpha,
        &["commit", "--allow-empty", "-q", "-m", "nothing changed"],
    );
    let mut harness = log_harness(&seed);

    // 1. Nothing selected.
    assert_painted(&harness, "Select a commit to see its changed files.");

    // 2. A filter that hides every row of a commit that has rows.
    select_docs_commit(&mut harness, &seed);
    harness.get_by_label("Filter changed files").click();
    harness
        .get_by_label("Filter changed files")
        .type_text("zzz");
    settle_pumped(&mut harness);
    assert_painted(&harness, "No file matches the filter.");
    assert_not_painted(&harness, "No changed files.");

    // 3. A commit with no changed files at all.
    let head = head_of(&seed);
    harness
        .get_by_label(&format!("{} nothing changed", short(&head)))
        .click();
    settle_pumped(&mut harness);
    assert_painted(&harness, "No changed files.");
    assert_not_painted(&harness, "No file matches the filter.");
}

/// `alpha`'s current HEAD — the commit the fixture appends after `seeded_project`.
fn head_of(seed: &Seed) -> String {
    run_git(&seed.alpha, &["rev-parse", "HEAD"])
        .trim()
        .to_string()
}

// --- Redesign issue 05: the commit-details pane ------------------------------

/// The bottom band of the right column — the commit-details pane. Its SURFACE
/// fill at the right column's width is the only rect of that shape.
fn details_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::SURFACE && r.width() >= 334.0 && r.width() <= 354.0)
        .map(|(r, _)| r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("details pane band not painted")
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
fn the_author_card_replaces_the_combined_author_row() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let painted = painted_in_region(&harness, details_region(&harness));
    assert!(
        painted.iter().any(|t| t == "T"),
        "the avatar carries the author's initials: {painted:?}"
    );
    assert!(painted.iter().any(|t| t == "Test"), "name: {painted:?}");
    assert!(
        painted.iter().any(|t| t == "test@example.com"),
        "email: {painted:?}"
    );
    assert_eq!(
        painted.iter().filter(|t| t.starts_with("Test <")).count(),
        1,
        "the committer grid row is the only `Name <email>` value left: {painted:?}"
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
    // The bar itself: a filled track carrying a success segment.
    let region = details_region(&harness);
    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(r, c)| *c == Palette::STATE_SUCCESS && region.contains(r.center())),
        "the churn bar paints its added segment"
    );
}

#[test]
fn all_four_detail_verbs_stay_reachable_and_the_guardrail_still_gates_them() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let painted = painted_in_region(&harness, details_region(&harness));
    for verb in [
        "Cherry-pick to…",
        "Cherry-pick across…",
        "Revert commit",
        "Create branch here",
    ] {
        assert!(
            painted.iter().any(|t| t == verb),
            "`{verb}` must stay reachable (decision D2): {painted:?}"
        );
    }

    // The guardrail still gates the disabled verb…
    harness.get_by_label("Revert commit").click();
    settle_pumped(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "revert stays gated while the current branch is protected"
    );

    // …and the ghost grid's other verbs still land as deferred dialogs.
    harness.get_by_label("Create branch here").click();
    settle_pumped(&mut harness);
    assert!(
        matches!(harness.state().ui.dialog, Some(Dialog::NewBranch)),
        "the click must still open the new-branch dialog"
    );
}

#[test]
fn the_guardrail_reason_renders_inside_an_alert_box() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    // `main` is protected by the default settings, so a reason is always on.
    let pos = galley_origin(&harness, "'main' is a protected branch — revert blocked")
        .expect("the guardrail text is still stated");
    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(rect, fill)| *fill == Palette::SURFACE_WARNING && rect.contains(pos)),
        "the reason must sit on the warning surface, not float as a bare label"
    );
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

    // The name is a line of its own and the directory is the line under it —
    // the path is never cut in the middle of a name.
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
    // The count is a chip of its own: a "1" galley sitting on a filled pill.
    let pos = galley_origin(&harness, "1").expect("count pill painted");
    assert!(
        filled_rects(&harness)
            .into_iter()
            .any(|(rect, _)| rect.contains(pos)),
        "the count must sit on a filled chip"
    );
}

// --- Issue #19: path-scoped file history from the log context menu ------------

/// Drive the full user path: select `row_label`, right-click its changed-file
/// entry `file`, and activate "Show history for file..." in the context menu.
fn scope_log_to_file(harness: &mut Harness<'_, AppState>, row_label: &str, file: &str) {
    harness.get_by_label(row_label).click();
    settle(harness);
    harness.get_by_label(file).click_secondary();
    settle(harness);
    harness.get_by_label("Show history for file...").click();
    settle(harness);
}

#[test]
fn show_history_for_file_scopes_the_log_to_touching_commits() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // Unscoped: every alpha commit plus beta's are painted.
    assert_painted(&harness, "alpha: docs commit");
    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "beta: root commit");

    // Right-click README.md on the docs commit → scope the log to that path.
    let docs_label = format!("{} alpha: docs commit", short(&seed.c3));
    scope_log_to_file(&mut harness, &docs_label, "README.md");

    assert!(
        harness.state().ui.log_path_scope.is_some(),
        "activating the action must record the path scope"
    );
    // ONLY commits touching README.md remain visible.
    assert_painted(&harness, "alpha: docs commit");
    assert_not_painted(&harness, "alpha: second commit");
    assert_not_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "beta: root commit");
}

#[test]
fn scoped_history_keeps_graph_and_details_functional() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // Scope to file.txt from the second commit's changed-file entry.
    let label = format!("{} alpha: second commit", short(&seed.c2));
    scope_log_to_file(&mut harness, &label, "file.txt");

    // Scoped listing: both file.txt commits, nothing else.
    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "alpha: docs commit");
    assert_not_painted(&harness, "beta: root commit");

    // Graph interaction still works inside the scope: clicking a scoped row
    // feeds the details pane…
    let scoped_row = format!("{} alpha: initial commit", short(&seed.c1));
    harness.get_by_label(&scoped_row).click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "rows inside the scope must stay selectable"
    );
    assert_painted(&harness, &short(&seed.c1));
    assert_painted(&harness, "click to copy full hash");
    assert_painted(&harness, "Test");
    assert_painted(&harness, "Parents");

    // …and the changed-files pane still lists the selected commit's files.
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

    // Issue 17: the scope renders as a chip with a removable × in the log
    // toolbar (replacing the old "Clear path history" banner button).
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

    // Contract step: the legacy History tab is deleted from the strip —
    // and Settings left it too (issue #16, gear-only modal now).
    assert_not_painted(&harness, "History");
    assert_not_painted(&harness, "Settings");

    // Every remaining tab is reachable and lands on its tool window.
    // ("Commit" is intentionally reached by keyboard only — its label also
    // exists on the toolbar button, and kittest rejects ambiguous queries.)
    harness.get_by_label("Log").click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.tab,
        Tab::Log,
        "Log tab must land on its tool window"
    );

    // Keyboard navigation stays valid: Ctrl+K always lands on Commit.
    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Commit);
}

// --- ticket 02: the current branch's one treatment -----------------------------

/// The Log pane renders the same tree component with `shows_row_actions:
/// false`, so the current branch must be marked there too — and only there, in
/// the branches pane, not smeared across the graph or the details column.
#[test]
fn the_branches_pane_marks_the_current_branch_with_its_badge() {
    let seed = seeded_project();
    let harness = log_harness(&seed);
    let pane = branches_region(&harness);

    let (in_pane, elsewhere) = harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == "current" => Some(shape.pos),
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
        "the Log pane's branch rows carry the current badge; painted galleys: {:?}",
        painted_text(&harness)
    );
    assert_eq!(
        elsewhere, 0,
        "the current badge belongs to the branches pane, not the rest of the Log window"
    );
}
