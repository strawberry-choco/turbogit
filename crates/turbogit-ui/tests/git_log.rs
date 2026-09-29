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
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use tempfile::TempDir;
use test_support::harness::{
    assert_menu_item_gated, click_menu_item, filled_circles, right_click_row, stroked_rects,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
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

    // Pane headers (uppercase micro-headers per spec §3.3/§8.3) — the shared
    // pane header, so the commit table is headed like every other pane too.
    assert_painted(&harness, "BRANCHES");
    assert_painted(&harness, "COMMITS");
    assert_painted(&harness, "CHANGED FILES");
    assert_painted(&harness, "COMMIT DETAILS");
    assert_painted(&harness, "ROOTS");

    // Live search inputs in both panes (placeholders as accessible labels).
    assert_painted(&harness, "Search branches");
    assert_painted(&harness, "Search commits");

    // Branches pane: the ~210px **card** at the far left of the body. Its
    // surface is `CONTENT_BG` and it strokes nothing (R2); the geometry — a
    // 210px column hugging the body's left edge — is what this file has always
    // asserted and is unchanged.
    let branches = branches_region(&harness);
    assert!(
        (branches.left() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 24.0,
        "branches pane must hug the left edge of the log body (right of \
         the workspace sidebar): left={:?}",
        branches.left()
    );

    // Right column: ~344px wide, reaching the body's right edge (decision D3
    // widened the §8.3 320px for the two-line file rows). The metadata rail
    // that once occupied the last 260px was removed in the local-changes
    // redesign (issue 03), so the right column extends to the window edge like
    // the pre-rail layout. It is **two cards** now — changed files on top,
    // details pinned below — sharing the column's width and separated by air.
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

    // The details pane's own height is the design value D4 sized (440px, with
    // the actions and the alert it covered since removed — the height is left
    // where it is rather than re-fitted). It is pinned to the bottom of the
    // column, above nothing and below the files card.
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

// --- Cycle 2: refs collapse into one label pill; names show on hover -------

// **Name collision, deliberately kept:** this test *name* contains the retired
// `widgets::ref_label` as a substring, but it never called it. It asserts the
// log surface's own hand-painted label pill — the one that collapses several
// refs into a single pill and expands them on hover. A blind name search for
// `ref_label` in the dead-widget sweep would have deleted live coverage.
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
                mode: LogBatchMode::Replace,
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

    // The redesigned blocks: the hash still on show, an author card, and the
    // meta grid's aligned labels. Its click-to-copy caption went with the click
    // (ADR-0024) — the hash is information the pane keeps, copying is an action
    // the menu owns.
    assert_painted(&harness, &short(&seed.c2));
    assert_not_painted(&harness, "click to copy full hash");
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

    // Some galley of the selected subject sits inside the translucent
    // `selection_bg()` focus band…
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

/// The ~210px card of the branches pane — the log's left column.
///
/// The surface token is `CONTENT_BG` rather than the `SURFACE` the pane wore
/// before design-system v2: the branches, files and details panes are cards now
/// (R2 — a content region is a surface, not a box, and it strokes nothing), and
/// this is the narrow one at the far left. The geometry is unchanged; only the
/// surface followed the pane.
fn branches_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .find(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 200.0 && r.width() <= 220.0)
        .map(|(r, _)| r)
        .expect("branches pane card not painted")
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

// **Name collision, deliberately kept:** this test *name* contains the retired
// `widgets::status_badge` as a substring, but it never called it. What it
// asserts is the log changed-files pane's own file-status letter chips, which
// are the branch/log feature role, not the retired screens-gap status-badge
// family. A blind name search for `status_badge` in the dead-widget sweep would
// have deleted live coverage.
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

    // The pill is the shared chip's height and text inset, painted through the
    // shared painter-level chip (issue 07). Pinning both here is what fails if a
    // change hands this row its own fill and centring back, or retargets it at a
    // different chip geometry.
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
    // Centred on both axes, exactly as the shared chip's text origin places it.
    assert!((pos.x - (badge.0.center().x - galley.size().x / 2.0)).abs() < 0.01);
    assert!((pos.y - (badge.0.center().y - galley.size().y / 2.0)).abs() < 0.01);
    // It keeps this row's own compact radius: the shared *geometry* with
    // CONTROL_RADIUS, not the full-height pill radius. Switching the badge to
    // `CHIP_GEOMETRY` wholesale would be a visible design change, so the radius
    // is pinned here to keep the consolidation honest.
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

/// The bottom card of the right column — the commit-details pane.
///
/// It is the **lower** of the two `CONTENT_BG` cards the column is made of (the
/// files pane's is above it), which is what identifies it: the details pane is
/// pinned to the bottom of the column, so "the later one" and "the details pane"
/// are the same rect. Its surface was `SURFACE` before this ticket; the card is
/// `CONTENT_BG` with no stroke (R2).
fn details_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 334.0 && r.width() <= 354.0)
        .map(|(r, _)| r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("details pane card not painted")
}

/// The upper of the right column's two cards — the changed-files pane.
fn files_region(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::CONTENT_BG && r.width() >= 334.0 && r.width() <= 354.0)
        .map(|(r, _)| r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("changed-files pane card not painted")
}

/// The **commits pane's card** — the one region this suite used to say was not a
/// card at all.
///
/// Found by **position**, not by a width, and that is the whole point of it: the
/// graph is the leftover column, so its width is a function of the window and of
/// the two side columns' widths and is not a number anything can assert against.
/// What is stable is that a carded commits pane sits between the branches card
/// and the right column's, and that is what identifies it. A graph pane that had
/// gone back to being bare body would paint no `CONTENT_BG` rect here at all, so
/// this helper is the premise for every gutter assertion below rather than a
/// convenience.
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
/// across the gap — "the cards do not overlap" is not "the cards are separated".
/// So this walks the painted columns and asks, of each one, what is actually
/// drawn there. A column counts only if every fill covering it is
/// [`Palette::BG`], which is the app background the central panel paints behind
/// the log body, and coverage is half-open (`min < x + 1 && x < max`) so that a
/// card *ending* at the column's edge is not reported as covering it: the
/// question is what is inside the gutter, not where its neighbour stops. A
/// fully transparent rect is not a fill and is skipped, exactly as the rest of
/// this suite's rect probes skip it — egui emits one per un-framed panel.
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

/// Whether a stroked rect is painted as a log pane card's **own edge**.
///
/// A stroke on a region is the frame drawn around it, so it is centred on the
/// region: `egui` folds the stroke width into the frame's inner margin and emits
/// one `Shape::Rect` carrying both the fill and the stroke at the same rect, so
/// the two are equal and a rule drawn along the boundary is the same shape again.
/// Both are caught by the same question — *is this stroke the card, rather than
/// something inside it?* — which is "the stroke is centred on the card and is
/// not tucked inside it with a hairline to spare".
///
/// Two neighbours it deliberately does not flag, and both are about whose stroke
/// this is. A control that lives **inside** the card: the log's two search inputs
/// stroke their own frames, the roles contract exempts a control's border by
/// name, and they are wholly contained by the card. And a surface in the pane
/// next door whose rect merely **abuts** the card's edge without reaching into
/// it — the workspace sidebar's rows end exactly where the branches card begins,
/// so the pair are edge to edge and a containment test alone would call the
/// sidebar's stroke the card's. Requiring the stroke's own centre to be inside
/// the card is what tells those two apart from a hairline, which is a whole point
/// wide and centred on the region it belongs to.
fn crosses_card(stroked: Rect, card: Rect) -> bool {
    const EPS: f32 = 0.5;
    stroked.intersects(card)
        && card.contains(stroked.center())
        && !card.shrink(EPS).contains_rect(stroked)
}

/// Every vertical line the frame paints, as `(x, y_range)` — the population, so
/// a test can see that a divider is *gone* and not merely moved.
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

/// **The details pane states the author once, as a key/value row** — the reshape
/// of the old author card, which said the name and the email in prose and then
/// said the same two things again as labelled grid rows.
///
/// The pane's empty state, its subject and its churn summary are unchanged; what
/// is asserted here is that the author is one `KEY value` line rather than a
/// card plus a row, and that the card's avatar is gone with it — an avatar
/// beside a row that already says who the author is states the same fact twice,
/// and the key/value list is the shape the rest of the pane now uses.
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
    // The bar itself: a filled track carrying a success segment.
    let region = details_region(&harness);
    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(r, c)| *c == Palette::STATE_SUCCESS && region.contains(r.center())),
        "the churn bar paints its added segment"
    );
}

// --- ticket 09: the changed-file rows share the one menu host -------------------

/// Every `.rs` path under a directory.
fn walk_rs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_rs(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// Right-clicking a changed-file row opens the SAME shared surface every other
/// menu in the window uses — the shared frame, the shared row primitive, and the
/// shared rule that a blocked item states its reason — rather than a third menu
/// dialect of raw buttons in a stock frame.
#[test]
fn a_changed_file_row_opens_the_shared_menu_and_its_verbs_still_work() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    right_click_row(&mut harness, "file.txt");

    // The shared surface's own signature: the popup frame the menu primitives
    // paint, which a stock egui menu does not.
    assert!(
        stroked_rects(&harness)
            .iter()
            .any(|(_, stroke, _)| *stroke == Palette::LINE),
        "the row's menu paints inside the shared surface frame"
    );
    assert_painted(&harness, "Show blame");
    assert_painted(&harness, "Show history for file…");

    // Show blame still blames the file at the selected commit.
    click_menu_item(&mut harness, "Show blame", "Show blame");
    settle_pumped(&mut harness);
    let blame = harness.state().ui.blame.clone().expect("blame opened");
    assert_eq!(blame.path, PathBuf::from("file.txt"));
    assert_eq!(blame.rev, seed.c2, "blamed at the commit that was selected");

    // Show history for file… still scopes the whole workspace to the file.
    right_click_row(&mut harness, "file.txt");
    click_menu_item(&mut harness, "Show blame", "Show history for file…");
    settle_pumped(&mut harness);
    assert_eq!(
        harness.state().ui.log_path_scope,
        Some(PathBuf::from("file.txt")),
        "the row's other verb still does what it did"
    );
}

/// A blocked item on a changed-file row stays rendered and explains itself: the
/// whole window follows one rule for why something cannot be done. Scoping a
/// file's history while the workspace is already scoped to that file is the case
/// the row can actually be in.
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

/// The stock-egui menu dialect is gone from the codebase: no surface builds a
/// context menu out of raw buttons in a stock frame any more, so a third dialect
/// cannot creep back one file over from the second.
#[test]
fn no_surface_builds_a_context_menu_out_of_stock_egui_anymore() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let offenders: Vec<String> = walk_rs(&src)
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

/// The four verbs the details pane used to perform are reachable from the
/// commit's menu instead, and the guardrail gates the same one there. Nothing
/// was dropped on the floor: the pane lost its actions, the menu gained them.
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
        // The pane called it "Create branch here"; the menu item is "New branch".
        "New branch",
    ] {
        assert_painted(&harness, verb);
    }

    // The guardrail gates the disabled verb the same way the button did: `main`
    // is protected by the default settings, so a blocked revert never reaches
    // its confirmation.
    assert_menu_item_gated(&mut harness, "Copy hash", "Revert commit");
    click_menu_item(&mut harness, "Copy hash", "Revert commit");
    settle_pumped(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "revert stays gated while the current branch is protected"
    );

    // …and the verbs that are not gated still land as their dialogs.
    right_click_row(&mut harness, &row_label);
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle_pumped(&mut harness);
    assert!(
        matches!(harness.state().ui.dialog, Some(Dialog::NewBranch)),
        "the click must still open the new-branch dialog"
    );
}

/// The guardrail reasons left the alert box with the box itself and now sit on
/// the items they block — the pane no longer warns about actions it does not
/// offer, and one rule answers "why can't I do this" across the window.
#[test]
fn the_guardrail_reason_now_sits_on_the_item_it_blocks() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    select_second_commit(&mut harness, &seed);
    assert_not_painted(&harness, "'main' is a protected branch — revert blocked");

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
/// entry, and activate "Show history for file…" on the shared menu. The label's
/// ellipsis is one character now that the row shares every other menu's wording;
/// the stock three-dot form died with the dialect it came from (ticket 09).
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

/// The Log pane renders the same tree component as the Branches tab, without
/// its context menu, so the current branch must be marked there too — and only
/// there, in the branches pane, not smeared across the graph or the details
/// column.
///
/// **What moved, and why.** The marker used to be a filled pill printing the
/// lowercase word `current`. Ticket 16 replaced it with the word `Current` in
/// the accent text ink on no fill (a filled marker on the current row would be
/// the same opaque value a chosen row takes), so the marker is now *words*
/// rather than a badge. The claim this test makes — one current marker, in the
/// branches pane, nowhere else in the window — is unchanged; only the string it
/// finds changed, and it is found in the component's own output so the Log pane
/// inherits the treatment automatically.
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
            Shape::Text(shape) if shape.galley.text() == "Current" => Some(shape.pos),
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

// =============================================================================
// Design-system v2 — the log's pane chrome and the commit table's columns.
//
// R7 settled the vocabulary: one pane header (a 9px tracked title, an optional
// count chip, a right-aligned action slot, ONE `RULE_STRUCTURAL` hairline, then
// content), one column-header row (9px tracked `INK_3` labels over one
// structural underline), and cards that are surfaces rather than boxes. This
// block is where the LOG proves it wears that vocabulary, and the proof is
// painted geometry throughout — what the user sees, never which function was
// called, because the tests below have to survive the functions being renamed.
//
// The generic contracts live in `tests/widget_library.rs`: the column header's
// ink and underline are asserted there on a synthetic frame, and the pane
// header's own composition is asserted there. What is asserted **here** is the
// log-specific half the widget layer cannot see: that all four log panes put
// down the same header, that the commit table's *data* rows land on its *header*
// labels' boundaries, and that the three carded panes really are stroke-less
// cards with the details pane's raised parent gone.
// =============================================================================

use test_support::harness::{PaintedGalley, painted_galleys, painted_ink};
use turbogit_ui::theme::CARD_RADIUS;
use turbogit_ui::theme::TYPE_SECTION;
use turbogit_ui::ui::widgets::PANE_HEADER_HEIGHT;

/// The four log panes' titles, in the order they appear across the window.
const LOG_PANE_TITLES: [&str; 4] = ["BRANCHES", "COMMITS", "CHANGED FILES", "COMMIT DETAILS"];

/// The commit table's column-header labels, in reading order.
///
/// **The ROOTS column leads** (conformance issue 15), and it is *not* in this
/// list: the branches pane's ROOTS filter paints a group title with the same
/// word, so a frame-wide "painted once" count for `ROOTS` would be two. The
/// ROOTS header is therefore looked up inside the commit table's own region by
/// `roots_header_label`, which is also where a collision with the group title
/// would be invisible — the failure that matters is a second header *in the
/// table*, not a shared word in two panes.
const COLUMN_LABELS: [&str; 4] = ["HASH", "AUTHOR", "MESSAGE", "DATE"];

/// The commit table's header label for `label`, found **inside the commit table**
/// so a same-word label in another pane cannot satisfy or spoil the lookup.
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

/// Every 1px `RULE_STRUCTURAL` rect the frame painted, as `(rect, fill)`.
fn structural_rules(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RULE_STRUCTURAL)
        .map(|(rect, _)| rect)
        .filter(|r| (r.height() - 1.0).abs() < 0.01)
        .collect()
}

/// The one painted galley whose text is exactly `text`, or a panic naming it.
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

/// Every painted galley with its exact text, so a string that paints in several
/// panes can be told apart by where it is rather than by which one came first.
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

/// What one pane's header actually put down, measured against the pane's own
/// hairline rather than against the window: the numbers a comparison across
/// panes needs, because the four panes sit at four different places.
#[derive(Debug)]
struct PaintedHeader {
    title: String,
    /// The title's origin, relative to the header's own hairline.
    offset: Pos2,
    rule: Rect,
    /// The title's own type: the type size, the face, the paint-time ink, and
    /// the line height it laid out at (which is what "centred in the band" is
    /// measured against).
    size: f32,
    label_height: f32,
    family: egui::FontFamily,
    ink: Color32,
}

/// Measure one pane header off painted output: find the title, then the
/// structural hairline that belongs to it (the first one at or below the title
/// that starts where the title starts), and read the band back from it.
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

/// **All four log panes render the shared pane header, and none renders the log's
/// own header widget.**
///
/// The claim is proven by comparing the four headers *to each other* rather
/// than to a hand-written number: a pane that built its own header would have
/// to reproduce the other three exactly — the same band height, the same rule
/// position inside that band, the same type, the same ink — to pass, and a
/// header that reproduces all of that IS the shared header. A pane that fell
/// back to a bare section label fails on the missing hairline; a pane that grew
/// a 24px band fails on the offset; a pane that set its title at the control
/// size fails on the type.
#[test]
fn every_log_pane_wears_the_one_shared_pane_header() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    // Each render really is the pane it was asked for: a comparison of four
    // renders of the SAME pane would satisfy every assertion below.
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

    // The geometry, written down in points rather than left to the equality
    // above: the title is centred in a `PANE_HEADER_HEIGHT` band whose last
    // pixel is the hairline, it starts exactly where that hairline starts, and
    // the hairline is one pixel of the structural rule.
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

    // The type itself, measured rather than restated: 9px, the chrome face, the
    // muted ink. A pane that painted its title in `INK_4` would pass the
    // geometry and fail this.
    assert_eq!(first.size, TYPE_SECTION);
    assert_eq!(first.family, egui::FontFamily::Proportional);
    assert_eq!(first.ink, Palette::INK_3);
    assert_ne!(first.ink, Palette::INK_4);
}

/// The commit table's own band of the window: the pane header's hairline spans
/// it, so it is read off painted output rather than assumed.
fn graph_band(harness: &Harness<'_, AppState>) -> Rect {
    measure_pane_header(harness, "COMMITS").rule
}

/// **The commit table's column header reads HASH | AUTHOR | MESSAGE | DATE, in
/// that order, in the shared column chrome.**
///
/// Four galleys, one per column, on one baseline, left to right in that order —
/// asserted as geometry, because "a string usually paints more than once" makes
/// a text-presence check the wrong tool, and because the *order* is the reading
/// order the header exists to establish.
#[test]
fn the_commit_table_labels_its_columns_hash_author_message_date() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let labels: Vec<PaintedGalley> = COLUMN_LABELS
        .iter()
        .map(|label| the_galley(&harness, label))
        .collect();
    // One galley each: a label painted twice is a second header.
    for (label, hits) in COLUMN_LABELS
        .iter()
        .zip(COLUMN_LABELS.iter().map(|l| all_galleys(&harness, l).len()))
    {
        assert_eq!(
            hits, 1,
            "the `{label}` column header paints {hits} times; a column is labelled once"
        );
    }

    // One baseline: the four labels share a row.
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
    // Reading order, left to right.
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

    // The labels sit ABOVE the data, not among it: the header's own hairline is
    // under them, and the first row is under that.
    let graph = graph_band(&harness);
    // "Below the labels" is the labels' TOP, not their middle: the shared
    // column chrome allocates its band and then paints the labels against a
    // separately-read top, so its rule currently lands a few points into the
    // label's own line box. That is the shared function's arithmetic to fix
    // (`widgets::column_header`, and the assertion belongs beside it), and the
    // log's ratchet deliberately does not freeze the current value in place of
    // the right one — it pins the contract the log owns: one structural rule,
    // at the table's own left edge, under the labels and over the first row.
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

/// **Every column-header label paints in `INK_3` at 9px with tracking, and none
/// paints in `INK_4` — measured on the log's own render.**
///
/// `tests/widget_library.rs` holds the generic version of this, on a synthetic
/// frame with its own column table. This is the log's half: the same claim
/// about the ink the *Git Log window* puts on screen, which the widget layer
/// cannot see and which is the claim that actually matters here — the target
/// frames were drawn with the dim value before its contrast was checked, and a
/// reader comparing this window against a frame should find a test disagreeing
/// with the frame.
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
    // The type, off the laid-out font: 9px in the chrome face, and the same type
    // a pane title wears — a title and a column label are one tracked mark.
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

/// **The column-header row's underline is the structural hairline, it is one, and
/// it spans the table.**
///
/// The strength assertion is the log's own: the structural rule is the one that
/// gives a surface its structure, and the *footer* rule is the heavier step a
/// modal's action slot takes. A column header that underlined itself with the
/// footer rule would read as a dialog boundary in the middle of a table.
///
/// **The search is bounded to the commit table's own band, and the footer half of
/// this test already said so.** "Between the labels and the first row" is a
/// statement about *this table*, and read as a statement about the window it
/// sweeps the branches pane in as well — which has a header rule of its own, at
/// its own y, and a y-band is not a region. It cost a real failure: the log's
/// ten-point inter-column gutters moved the commit table down by one card
/// padding, and that was enough to drop the branches pane's header rule inside
/// the band, so a frame with exactly one commit-table underline reported two.
/// The bound is the same `graph` rect the footer check below already filters by,
/// and the assertion — exactly one, spanning the table — is untouched.
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

    // The band between the labels and the first row is where the underline lives.
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

    // …and it is the structural rule, not the footer rule. The footer rule is a
    // *distinct token*, so this is a real distinction rather than a tautology.
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

/// **Every column boundary in the header equals the corresponding boundary in the
/// data rows.**
///
/// This is the log-specific half of the column-chrome contract, and the reason
/// the header and the rows are compared to *each other* rather than to a table
/// of numbers: the failure this rule exists to prevent is a column that is
/// narrow in the header and wide in the rows, and the only thing that can see
/// that is one number in two places. So the test takes the header's four label
/// positions and the first data row's four cell positions off the paint and
/// requires them to be the same positions.
///
/// The trailing column shares an **edge** rather than a start, which is what a
/// right-aligned date cell means: the label ends where its cell ends.
#[test]
fn every_column_boundary_in_the_header_equals_its_boundary_in_the_rows() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    let graph = graph_band(&harness);
    let hash = the_galley(&harness, &short(&seed.c3));

    // The first data row's four cells, found by position rather than by text: a
    // commit row paints its cells straight at its column offsets, so the cells
    // on one row are exactly the columns, left to right. Restricted to the
    // graph band, because the files and details panes paint on the same rows.
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

    // The premise, stated: the row is the commit we asked for, and it painted
    // four cells — hash, author, subject, date. A row that painted a ref pill's
    // text or a different number of cells would make every comparison below
    // vacuous, so it is checked rather than assumed.
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

    // The three leading columns share their START edge with their labels.
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
    // The trailing column shares its END edge.
    let date_label = the_galley(&harness, "DATE");
    assert!(
        (date_label.rect.right() - cells[3].rect.right()).abs() < 0.01,
        "a trailing column's label ends where its cell ends: label {:?}, cell {:?}",
        date_label.rect,
        cells[3].rect
    );
}

/// Every painted filled rectangle with the rounding it was painted at, so a
/// card's radius is assertable from the paint rather than from the frame that
/// asked for it.
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

/// **All four log panes paint as cards with no stroke, and the details pane's
/// raised background parent is gone.**
///
/// Four claims, one frame. The card: the content surface at the card radius,
/// found by geometry rather than by a list of panes. The stroke: no stroked rect
/// shares any card's rect, which is the whole of R2 for a region that does not
/// float. The raised parent: the details pane used to be a `Panel::bottom` whose
/// frame filled `SURFACE`, sitting inside a `Panel::right` that filled `BG` —
/// a raised band wrapped around a bottom panel, on a background panel behind
/// that. The card replaces the nesting, so nothing in `SURFACE` may remain
/// behind the details card. And the **fourth** card: the commits pane.
///
/// **The count moved from three to four, and the reason is the inversion this
/// suite used to record as settled.** The old assertion read three, with the
/// note that the graph pane was deliberately not one of them because it was the
/// panel itself and "a card drawn over the whole remaining body would be a box
/// around a box". That was true of the layout as it stood: the graph pane *was*
/// the remaining body, so it had no edges of its own and a card over it could
/// only ever have been a box drawn around the panel. It stopped being true when
/// the columns stopped touching — the graph is now a region with
/// `PANE_COLUMN_GAP` of app background on either side, which is what gives it
/// edges, and a surface separated from its neighbours by air is one surface.
/// The count is a bound, and the bound moved because the design moved, not
/// because four turned out to be easier to satisfy than three: the two side
/// columns are still enumerated by name below and each must still paint.
#[test]
fn the_four_log_panes_are_stroke_less_cards_and_the_details_raised_parent_is_gone() {
    let seed = seeded_project();
    let mut state = AppState::new(seed.project.clone());
    warm_log_and_refs(&mut state);
    state.ui.tab = Tab::Log;
    state.ui.selected_commit = Some(seed.c3.clone());
    let mut harness = harness_with(state);
    settle(&mut harness);

    // The four cards: the content surface, tall enough to be a pane.
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

    // The radius, off the paint: a card at any other rounding is a panel.
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

    // **No stroke.** Not "no stroke of this colour" and not "no stroke on the
    // header": no stroked rect is any card's own edge. A card is a surface and
    // a stroke is the one signal the app has left for a surface that floats above
    // its surroundings, and none of these four float. Controls that sit inside a
    // card keep their own borders — see [`crosses_card`], which is where that
    // distinction is drawn.
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

    // **And nothing is stroked in either gutter.** The two side panels used to
    // leave egui's 1px separator showing at their inner edges, which is the edge
    // of a content region wearing a stroke — the one thing R2 rules out, and the
    // literal "1px dividers" the pre-gutter layout drew. The air replaces it.
    //
    // The bound is the **whole gutter, both of its edges included**, and that is
    // the version that survived being broken: bound to the cards' edges alone, it
    // passed with the separator switched back on, because the gutter is now
    // reserved inside the panel rather than beside it, so egui draws the line
    // half a point past the panel's own boundary — in the middle of the air,
    // where it divides nothing and still reads as a second border. A rule in a
    // gutter is a rule in a gutter wherever it falls inside it.
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

    // **The details pane's raised background parent is gone.** Nothing in
    // `SURFACE` may sit behind or over the details card any more: that raised
    // band was the parent this ticket retired.
    // The height floor is what makes this a claim about the *parent*: the
    // retired band was 440 points of `SURFACE` around the pane, while the
    // controls that legitimately sit on a card (a count chip) are one row tall.
    // Without the floor the app's own status bar, which abuts the details card's
    // bottom edge, would be reported as a parent.
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
    // …and the two right-column cards divide by air, so one does not sit on the
    // other. A single pixel of gap is what R2 asks for, not a rule and not a
    // second surface.
    assert!(
        details.top() >= files.bottom(),
        "the details card sits below the changed-files card, not over it: \
         files {files:?}, details {details:?}"
    );
}

/// **The log's three columns are separated by ten points of app background, and
/// the commits pane is a card so that it reads as their peer.**
///
/// **The frame this is measured against.** The approved Log target
/// (`design-exports/02-log.png`, a 2x export of a 1440x900 window) was scanned on
/// a horizontal line through the tool-pane band and the two outer cards came back
/// as `#232529` — `Palette::CONTENT_BG`, which is what the shipped render already
/// wore. What it did **not** show was any background between them: at the frame's
/// own x, the shipped pixel was still card fill, because the columns were drawn
/// edge to edge with a 1px divider standing in for the separation. Ten points of
/// `#1E1F22` — the app background — is the whole of the difference, plus the
/// commits pane becoming a card at all.
///
/// **Three separate claims, and each one can fail alone.**
/// 1. **Separation.** Both gaps are at least eight points wide — the floor R2
///    sets for "these are distinct regions" and the number a mutation has to beat.
/// 2. **The frame's number.** Both gaps are *exactly* `PANE_COLUMN_GAP`. A layout
///    that put the gap somewhere else — inside the right column, say, or as
///    panel padding — would pass the floor and still be the wrong rhythm.
/// 3. **The gap is air.** Measured off a scan line through the painted output, not
///    off the card rects: every one-point column of each gap carries
///    [`Palette::BG`] and nothing else. This is the assertion that "they are
///    separated" cannot be satisfied by three cards that merely touch, and it is
///    the one that fails if a fourth surface is ever painted across the gap.
#[test]
fn the_three_log_panes_are_separated_by_ten_points_of_app_background() {
    let seed = seeded_project();
    let harness = log_harness(&seed);

    // The premise: the commits pane is a card, so there are three cards to
    // separate. A graph pane that had gone back to bare body paints none.
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

    /// R2's floor for "these are two regions and not one": a card divides itself
    /// from its neighbour by air, and a mutation has to beat this to slip past.
    const MIN_GUTTER: f32 = 8.0;
    /// The frame's own inter-column gutter, in points. Kept local and explicit
    /// rather than imported: the constant the production code reads is
    /// `log_window::PANE_COLUMN_GAP`, and a test that read the value it is
    /// checking would agree with any change to it, including deleting it.
    const FRAME_GUTTER: f32 = 10.0;

    // A scan line through the middle of the tool-pane band, clear of the pane
    // headers, the column header rule and the card corners, so what the columns
    // report is the band and not one of its own rules.
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

        // Claim 3, measured from the paint rather than from the layout.
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

/// The log window's own source, read for the places it is allowed to make an
/// ink decision.
const LOG_SRC: &str = include_str!("../src/ui/log_window.rs");

/// Every spelling of a colour that is not a token. A hex literal in the log is a
/// colour that answers to nothing and drifts the moment the token moves, so each
/// function that is *supposed* to take its ink from the shared ramp is checked
/// against this list — the chrome's two functions and the rows' four.
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

/// One top-level `fn`'s source text, from its signature to the first column-zero
/// `}` after it.
fn fn_source(src: &str, name: &str) -> String {
    let start = src
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no `fn {name}` in the log window"));
    let rest = &src[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`fn {name}` has no closing brace at column zero"))
        + 3;
    rest[..end].to_owned()
}

/// **The log's chrome takes its ink from the shared ramp: no local muted-ink
/// literal remains in the header cells or the micro-text helper.**
///
/// Two seams, on purpose. The *source* seam pins the negative — a hex literal
/// in either of those two functions is a colour that answers to no token and
/// drifts the moment a token moves, and a render seam can only prove that at
/// the call sites a test enumerates, of which there are none here (both
/// functions are the whole vocabulary). The *render* seam then pins what the
/// micro-text helper actually resolves to on screen, which is the half that
/// matters: the helper is named as though it belonged to the column header and
/// it does not — every caller is the details pane's — so a reader who changes
/// its ink is changing a screen nobody was looking at. Asserting what it
/// resolves to is what makes that visible.
#[test]
fn the_logs_chrome_names_no_ink_outside_the_shared_ramp() {
    for (name, src) in [
        ("micro_text", fn_source(LOG_SRC, "micro_text")),
        ("header_cells", fn_source(LOG_SRC, "header_cells")),
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

    // The render half: the details pane's **keys** are the muted step, and the
    // dim step is not an option for them. They are a commit's facts — who
    // authored it, when, who committed it, what it descends from — and the dim
    // step fails AA, so it may never be the only rendering of something the user
    // needs.
    //
    // The list is the *keys* and nothing else: since the key/value reshape every
    // caller of the micro-text helper is a key, an em dash standing in for a
    // value the commit lacks, or the "N files changed" count. Asserting a
    // **value** here would be asserting the opposite role, which is what
    // `the_details_pane_lists_author_date_and_parents_above_the_message_body`
    // does.
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

// --- Conformance issue 14: the commit rows are the shared row --------------------
//
// Everything below reads the log through the production render path and speaks
// only in painted output and public state. The claims being pinned, one test
// each:
//
// 1. a chosen commit row paints the shared row fill — the one row-fill
//    decision's answer for the log's band, which the token layer defines as the
//    opaque equivalent of the selected-list-row fill — **and** the one 2px brand
//    rail at its leading edge, and nothing fills with the current-ref or accent
//    token;
// 2. its text is the same colour as the same cell on an unchosen row, starts at
//    the same x, and no row in the window indents when it is chosen — selection
//    is paint, not a second palette;
// 3. the per-root stripe and the rail share the leading gutter, both visible, and
//    the gutter is the same number whether or not the row is chosen;
// 4. the row's own ink comes from the shared ramp, the collapsed ref marker is
//    the neutral badge rather than a second ref-chip painter, and the row still
//    arrives at the shell as a rect the log allocated itself.

/// The label of a commit row in the graph — the accessibility label the log gives
/// every row, and the handle every other suite in this file drives rows by.
fn commit_row_label(id: &str, subject: &str) -> String {
    format!("{} {subject}", short(id))
}

/// The commit table's own band: between the branches pane and the right column.
///
/// Tighter than [`graph_region`], because the claims here are about what *this*
/// table paints and the right column's cards have selections of their own. The
/// right edge is read off the changed-files card rather than estimated, so a
/// change to either pane's width cannot quietly widen the scope of a ratchet.
/// The commit table's own region: the commits **card**, and specifically the
/// content inside it.
///
/// **This used to be an approximation, and the card retired it.** It was
/// `branches.right() .. files.left()`, which describes the *leftover between two
/// panels* — true only while the graph pane was the leftover, and off by the
/// card padding and both inter-column gutters the moment it became a card. Every
/// containment probe built on it kept passing by luck (the slack went the right
/// way), while an assertion that measured *from* the region's left edge did not:
/// the ROOTS column's cell ends 5 points into the table, and the approximation
/// put the table's left edge 18 points to the left of where the table really
/// starts, so "within 8 points of the table's edge" read false for a swatch that
/// was exactly where it has always been.
///
/// So the region is read off the paint instead, and its horizontal range is the
/// **card's own pane-header rule**: the header is the first thing inside the
/// card, so that rule starts and ends where the content does. Deriving it that
/// way rather than shrinking the card by a restated `PANE_CARD_PAD` means this
/// cannot drift from the padding the card actually wears. The vertical range is
/// the card's, which is a superset of the content's — every probe that uses this
/// asks "is this inside the commit table", and none of them measures an offset
/// from the region's top edge.
fn commit_table_region(harness: &Harness<'_, AppState>) -> Rect {
    let card = commits_region(harness);
    Rect::from_x_y_ranges(graph_band(harness).x_range(), card.y_range())
}

/// Relative luminance and contrast, for the claims that are about legibility
/// rather than about identity. WCAG's formula, as the token layer states it.
fn luminance(color: Color32) -> f32 {
    let channel = |v: u8| {
        let c = f32::from(v) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// The WCAG contrast ratio of two opaque colours, for the claims that are about
/// legibility rather than about identity.
fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = (la.max(lb), la.min(lb));
    (hi + 0.05) / (lo + 0.05)
}

/// `Palette::selection_bg()` composited over `host` — the colour a chosen row
/// actually reads as on that surface.
///
/// The composite is computed rather than read off the frame because the claim is
/// arithmetic: `selection_bg()` is premultiplied, so the composite is the stored
/// bytes plus the host's contribution through the remaining alpha, and the token
/// layer names the result.
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

/// The filled rects painting exactly `color` inside `region`.
fn fills_in(harness: &Harness<'_, AppState>, region: Rect, color: Color32) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, painted)| *painted == color && region.intersect(*rect) == *rect)
        .map(|(rect, _)| rect)
        .collect()
}

/// **A chosen commit row paints the shared row fill and the one 2px brand rail at
/// its leading edge.**
///
/// The fill is the one row-fill decision's answer for the log's band,
/// `Palette::selection_bg()`, painted at the rect the row allocated — and the
/// token layer defines that composite's opaque equivalent as `ROW_SELECTED`, so
/// this *is* the row-selected fill, asserted as the arithmetic that says so
/// rather than as a name. The rail is the one rail painter's output: the token's
/// width, at the row's leading edge, for the row's full height.
#[test]
fn a_chosen_commit_row_paints_the_row_selected_fill_and_one_brand_rail() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_second_commit(&mut harness, &seed);

    let label = commit_row_label(&seed.c2, "alpha: second commit");
    let row = harness.get_by_label(&label).rect();
    let table = commit_table_region(&harness);

    // The band's identity: the one row-fill decision's focus fill, and nothing
    // else, at the caller's own rect.
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

    // …and its value: composited over this pane's surface, the log's chosen band
    // is the selected-list-row fill. (`selection_bg()` over the app background is
    // #233455; `ROW_SELECTED` is #243456.)
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

    // The rail: the token's width, at the row's leading edge, for its full
    // height, in the accent. This is the *only* brand-filled rect in the table
    // (asserted below), so "one rail for one selection" is a count, not a shape
    // someone recognised.
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

    // An unchosen row in the same frame carries no rail.
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

/// **No commit row fills with the current-ref token or the accent** — and the only
/// accent fill in the table is the one rail.
///
/// `SELECTION` is the current-ref band and belongs to `current_row_fill` alone; a
/// chosen list row takes the opaque fill. `BRAND` behind running text is the
/// solid band the row grammar replaced. Both are close in hue to the fill the log
/// actually paints, which is why this is asserted by token identity over every
/// filled rect in the table rather than by eyeballing one row.
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

/// The log's source with every comment blanked out, one space per character so
/// line structure survives.
///
/// Needed where a claim is about what the log's *code* does while the file's
/// documentation legitimately names the same values: a scan that counted a doc
/// comment would either fail on a well-argued comment or — worse — force the
/// argument to be deleted instead of written.
///
/// String literals are blanked too, so this is the seam for claims about
/// *identifiers* and *calls*. Where a claim is about a literal — a column's
/// label, a table's name — use [`code_without_comments`], which keeps them.
fn code_only(src: &str) -> String {
    blanked(src, false)
}

/// The log's source with comments blanked and string literals **kept**.
///
/// The same reasoning as [`code_only`] with the other half left standing: a
/// column's label and a table's name are string literals, so a scan that
/// blanked them could not see that a column exists at all.
fn code_without_comments(src: &str) -> String {
    blanked(src, true)
}

fn blanked(src: &str, keep_strings: bool) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let (mut i, mut block, mut line, mut string) = (0usize, false, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        // Whether this character arrives *inside* a comment — always masked — or
        // inside a string, which `code_without_comments` keeps and `code_only`
        // does not. Read before the transitions below, because a delimiter is
        // the first character of what it opens, not the last of what it closes.
        let masked = block || line || (string && !keep_strings);
        if block {
            if c == '*' && next == '/' {
                block = false;
                out.push(' ');
                out.push(' ');
                i += 2;
                continue;
            }
        } else if line {
            if c == '\n' {
                line = false;
                out.push('\n');
                i += 1;
                continue;
            }
        } else if string {
            if c == '"' {
                string = false;
                out.push(if keep_strings { '"' } else { ' ' });
                i += 1;
                continue;
            }
        } else if c == '/' && next == '/' {
            line = true;
            out.push(' ');
            out.push(' ');
            i += 2;
            continue;
        } else if c == '/' && next == '*' {
            block = true;
            out.push(' ');
            out.push(' ');
            i += 2;
            continue;
        } else if c == '"' {
            string = true;
            out.push(if keep_strings { '"' } else { ' ' });
            i += 1;
            continue;
        }
        // A character that is not inside a comment — and, for `code_only`, not
        // inside a string — is CODE, and is kept. Newlines always survive so line
        // structure (and therefore line-numbered failures) does too.
        //
        // **This is the fix.** The helper this replaced ended on a single
        // `push(if c == '\n' { '\n' } else { ' ' })`, which blanked the *code* as
        // well as the comments: the result was a file of newlines and spaces, so
        // every `!code_only(src).contains(needle)` scan over it was true for the
        // uninteresting reason that the needle could not possibly be there. A
        // ratchet that cannot fail is not a ratchet, and the one that exposed
        // this is the blame view's — where the scan for `CommitTable::ROW_HEIGHT`
        // passed on a file that had stopped calling it.
        out.push(if c == '\n' {
            '\n'
        } else if masked {
            ' '
        } else {
            c
        });
        i += 1;
    }
    out.into_iter().collect()
}

/// `fn_source` with the comments blanked — the seam for a claim about what a
/// function *calls*.
///
/// `fn_source` on its own returns the function's raw text, doc comment included,
/// which is the right seam for a claim about a *literal* the function must not
/// name (a doc comment that argued the point is still a comment) and the wrong
/// seam for a claim about a call: this file's own documentation names every
/// constant a function reads while explaining why it reads it, so an unblanked
/// scan for `CommitTable::ROW_HEIGHT` is satisfied forever by the prose.
fn fn_code(src: &str, name: &str) -> String {
    code_without_comments(&fn_source(src, name))
}

/// The commit table's column anchors, read from the **shared column-header row**
/// rather than from literals here — so a row's cells are matched to the header
/// that labels them, and a column that moved in one and not the other fails.
struct Columns {
    hash: f32,
    author: f32,
    message: f32,
    /// The date column is right-aligned to the row's trailing edge, so it is
    /// compared on its right edge like the cell it labels.
    date_right: f32,
}

fn commit_columns(harness: &Harness<'_, AppState>) -> Columns {
    let header = |needle: &str| {
        all_galleys(harness, needle)
            .into_iter()
            .find(|g| commit_table_region(harness).contains(g.pos))
            .unwrap_or_else(|| panic!("the column header `{needle}` paints in the table"))
    };
    // The three left-anchored columns are measured from the header label's own
    // leading edge; the date trails the row, so it is measured from the header
    // label's trailing edge — the same edge the cell it labels ends at.
    Columns {
        hash: header("HASH").pos.x,
        author: header("AUTHOR").pos.x,
        message: header("MESSAGE").pos.x,
        date_right: header("DATE").rect.right(),
    }
}

/// The one galley of `row`'s `column` cell, matched by the column's own anchor
/// rather than by string — a subject, a hash and a date are all different
/// strings per row, and a string usually paints more than once in a frame.
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

/// **A chosen commit row's text ink equals an unchosen row's**, and the same ink
/// clears AA on both surfaces the row can be on.
///
/// Position-scoped and column-by-column, never by first match: the two rows paint
/// different strings, and `painted_ink` alone would compare the wrong occurrence.
/// The values are asserted as well as the equality, because "equal" is satisfied
/// by two wrong colours just as happily as by two right ones.
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

    // The two surfaces a cell's ink has to read on: the panel the rows rest on,
    // and the band a chosen row takes — which the token layer defines as the
    // opaque equivalent of the selected-list-row fill.
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
        // The muted step is not legal on a chosen row's band, which is why the
        // two metadata cells are the secondary step and not the muted one. Named
        // here so a reader can see the rule the values above are answering.
        assert_ne!(
            on_chosen.color,
            Palette::INK_3,
            "{column}: the muted step is 3.76:1 on the band a chosen row takes"
        );
    }

    // And the negative that the whole ink ramp turns on: the accent is not an
    // ink for information. The hash is the cell that used to wear it, and it
    // measures 2.89:1 on the chosen band.
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

/// **A chosen row's text origin equals an unchosen row's** — the rail is paint,
/// never padding, and no column of the table moves when a row is selected.
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

    // The band is the row's own leading edge, so nothing is reserved beside it.
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

/// **The rail and the per-root stripe share the row's leading gutter**, both
/// visible, both where they were.
///
/// This is the decision conformance issue 14 had to make, pinned from painted
/// geometry: the rail leads flush at the row's leading edge (the one rail
/// painter's position) and the 3px root stripe sits immediately inside it, so a
/// chosen row still says which repository it came from. The gutter is
/// `RAIL_WIDTH + STRIPE_WIDTH` and nothing more, which is why the cells did not
/// move — and the stripe's position is the same on a chosen row and an unchosen
/// one, so nothing about the row's geometry depends on its state.
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

    // A row's stripe: a full-height 3px fill at the row's leading gutter, in the
    // root's own colour. Read by geometry rather than by a colour table the
    // module keeps private, and confirmed against the lane node in the same row
    // so the reader can see it is the root's colour and not a decoration.
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

    // Flush against the rail's trailing edge: rail, then stripe, then the cells.
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

    // The cells start clear of the gutter, in every row, chosen or not.
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

    // The stripe's colour is the root's, and the lane node in the same row wears
    // it too — one colour table, two markers, per root.
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

/// **Every row this window paints keeps its content where it was, chosen or not**
/// — including the two row kinds the commit table is not.
///
/// The commit table's own four columns are pinned above; this covers the
/// branches pane's ROOTS filter and the changed-files pane's file rows, which
/// reach the same shell and are the two places a "reserve the rail as padding"
/// implementation would show up without the commit table noticing.
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
    // Nothing is chosen in the filter yet, so "All roots" is a resting row.
    let all_roots_at_rest = roots_x(&harness, "All roots");
    let root_alpha_at_rest = roots_x(&harness, "Root alpha");

    // Choosing a different root: "All roots" becomes an unchosen row and
    // "Root alpha" the chosen one.
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

    // The changed-files pane: a file row's name, unchosen then chosen.
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

/// **The collapsed ref marker is the neutral badge, not a ref chip** — the answer
/// conformance issues 13/14 were asked to settle, pinned from both ends.
///
/// The render half: the marker in the graph is a **pill-slot** rounded rect of
/// the neutral badge's pair, in the same frame where the branches pane paints a
/// real ref chip at the **chip** radius. The two shapes are visibly different
/// objects, so the chip radius in this app means "a ref name" and the marker does
/// not quietly claim it.
///
/// The source half: the log names no chip value at all, so the chip set stays
/// closed at three with `widgets::ref_chip` as the only function that produces a
/// ref chip, and no painter-level sibling of it was smuggled in here instead.
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

    // The rounding is the half a `filled_rects` colour cannot show, and it is
    // the whole claim: the pill slot (PILL_RADIUS) here, the compact chip radius
    // (CHIP_RADIUS) on a real ref chip elsewhere in the same frame.
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
    // A real ref chip, in the same frame, at the chip radius: the shapes coexist
    // rather than one having been flattened into the other.
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

    // …and the source half, which is what "exactly one function produces a ref
    // chip" needs at this site: the log names no chip value and calls no chip
    // constructor, so a second ref-chip painter cannot be hiding behind the
    // painter-only path the virtualised row requires.
    //
    // Read from the *code*, comments blanked: this file's own reasoning names
    // `REF_CHIP_COLORS` on purpose, and a scan that counted a doc comment would
    // either fail here or force the reasoning to be deleted rather than written.
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

/// **The log's per-row ink comes from the shared ramp, not from a local helper.**
///
/// Two seams. The source seam pins the negative the render seam cannot: the log
/// declares no `row_ink(active)` any more, and the three functions that paint row
/// content name no colour literal — a literal is a colour that answers to no token
/// and drifts the moment the token moves. The render seam then pins what the rows
/// actually resolve to, which is the half that matters: the ramp is a claim, and a
/// claim nobody reads off painted output is a comment.
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
        ("row_name_ink", fn_source(LOG_SRC, "row_name_ink")),
        ("row_meta_ink", fn_source(LOG_SRC, "row_meta_ink")),
        ("commit_row", fn_source(LOG_SRC, "commit_row")),
        ("file_row", fn_source(LOG_SRC, "file_row")),
        ("paint_log_row", fn_source(LOG_SRC, "paint_log_row")),
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

    // The values, on screen. A row's identity text is the primary step; its
    // secondary cells are the secondary step — never the muted one, which is not
    // legal on any band a row can be in.
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

    // The ROOTS filter rows in the branches pane go through the same vocabulary.
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

// --- Conformance issue 15: the ROOTS column, the details pane -------------------
//
// The claims pinned here, one test each:
//
// 1. the ROOTS column is a column: its label is painted by the commit table's
//    own header row, out of the same `PaneColumn` table as the other four, in
//    the same ink, on the same baseline, at the row's leading edge;
// 2. the column, the graph lane node and the legend all paint `root_color(idx)`
//    — asserted by comparing the painted fills for one root, and by a source
//    scan that no colour literal exists outside the one table;
// 3. the column does not move the rail: the gutter is the rail *and* the swatch
//    in either kind of listing, and the cells sit where they sat before the
//    column existed;
// 4. the details pane lists author, date and parents as key/value pairs ABOVE
//    the message body, keys on the label ink and values on the value ink, and
//    its empty state still renders when nothing is selected;
// 5. the fact list is a new type over the commit the log holds, not the commit
//    menu's action gates.

/// **The ROOTS column has a header label, in the commit table's own header row,
/// out of the same column table as the other four.**
///
/// The geometry is the awkward part and it is stated rather than fudged. A 5px
/// column cannot hold a 9px word, so `ROOTS` is measured *backwards* from the
/// header row's origin by exactly the gutter ([`COL_ROOTS`]): it paints at the
/// row's leading edge, over the rail and the per-root swatch it names, and it
/// overhangs its own column. What that overhang may not do is reach the next
/// label, so that is the bound asserted here — a *fit* is not available at this
/// width and asserting one would be asserting a lie.
///
/// Four further claims are in the same frame, because "it is a header" is a
/// bundle: it is in this row (shared baseline with the other four), it is this
/// chrome (the same 9px chrome-face type a pane title wears, and `INK_3` —
/// never `INK_4`, because a column label is something the user reads), and it
/// comes from the same table (the source seam: `COMMIT_COLUMNS` names it, and
/// the only function that paints it is the shared `widgets::column_header`).
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

    // It is in the header row, not floating above it: one baseline with the
    // other four.
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
    // Reading order: ROOTS leads, because it is the leftmost column.
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

    // The ink and the type are the shared chrome's, not a second header's.
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

    // At the row's leading edge, over the column it names — and clear of the
    // next label, which is the bound a 9px word in a 5px column actually has.
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

    // The source seam: the label is an entry in the table the header row reads,
    // and the header row is the shared one. A free-floating label painted
    // somewhere else would carry none of the chrome's ink and would sit outside
    // the header's rule — which is precisely what a `PaneColumn` entry does not.
    let with_strings = code_without_comments(LOG_SRC);
    assert!(
        with_strings.contains(r#"PaneColumn::start("ROOTS""#),
        "the ROOTS label must be an entry of the commit table's own column \
         table, not a label painted free-floating above the gutter"
    );
    let header = fn_source(LOG_SRC, "header_cells");
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

/// The ROOTS column's cell on `row` — the gutter swatch, found by geometry
/// rather than by a colour table the module keeps private: a full-height fill at
/// the row's leading gutter, 3px wide, in some colour.
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

/// **The graph lane node, the legend swatch and the ROOTS column all paint the
/// same colour for a given root** — and none of them can name a colour of its
/// own.
///
/// The render half compares the *painted fills* rather than the names beside
/// them, because two swatches can both be labelled `alpha` while one of them has
/// drifted to a second palette. The legend entry for a root is found by its
/// name's own position (the swatch is laid out immediately before the label in
/// the legend's flow), the column's cell by the gutter geometry, and the lane
/// node by the row it is centred on.
///
/// The source half pins the negative a render seam cannot reach: the log spells
/// no colour literal anywhere outside `GRAPH_COLORS`, and every per-root marker
/// asks `root_color`.
#[test]
fn the_roots_column_the_lane_and_the_legend_paint_one_colour_per_root() {
    // ---- the source seam --------------------------------------------------
    let code = code_without_comments(LOG_SRC);
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
    // …and the four markers are four *call sites* into it, not four copies of it.
    for (name, source) in [
        ("paint_root_swatch", fn_code(LOG_SRC, "paint_root_swatch")),
        ("commit_row", fn_code(LOG_SRC, "commit_row")),
        ("graph_pane", fn_code(LOG_SRC, "graph_pane")),
        (
            "roots_filter_section",
            fn_code(LOG_SRC, "roots_filter_section"),
        ),
    ] {
        assert!(
            source.contains("root_color("),
            "`fn {name}` paints a per-root marker, so it must ask `root_color` for \
             the colour rather than carrying one:\n{source}"
        );
    }

    // ---- the render seam --------------------------------------------------
    //
    // Compared by *painted fill*, and per root rather than per shape: a row is a
    // commit from exactly one repository, so the ROOTS column's cell, the graph
    // lane node painted on that same row, and the legend entry for the root that
    // row belongs to must all be the one colour. The root each row belongs to is
    // read from the app's own cache (the row's own hash, which is what its
    // accessibility label is built from) rather than guessed from its position in
    // the listing — a listing sorted by time does not keep roots in blocks.
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    settle(&mut harness);
    let table = commit_table_region(&harness);
    let header_top = header_label(&harness, "HASH").pos.y;

    // The legend: one 10x10 swatch per root, laid out left to right in
    // `state.multi.roots` order in a single wrapped flow, in the band above the
    // column header row.
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
        // This root's own newest commit, and therefore the row it is on.
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
        // Stated so the agreement cannot pass vacuously: it is a real colour and
        // not the surface showing through the gutter.
        assert_ne!(
            cell_color,
            Palette::BG,
            "the gutter swatch is a root colour, not the pane's surface"
        );
        seen.push(cell_color);
    }
    // …and so the ratchet cannot be satisfied by a table of one: two roots, two
    // colours, agreed on in all three places.
    assert_eq!(
        seen.iter().collect::<std::collections::HashSet<_>>().len(),
        root_count,
        "each root gets its own colour out of the one table: {seen:?}"
    );
}

/// **The ROOTS column never moves the rail** — the rail-versus-stripe decision
/// conformance issue 14 made, re-read and held against the column issue 15 put
/// on top of it.
///
/// Three frames' worth of claim, because "it did not move" is only interesting
/// against the two things that could have moved it. **On the chosen row** the
/// rail is still flush at the leading edge with the swatch immediately inside
/// it, so the row the user is reading keeps its root colour. **Across kinds of
/// listing** the rail is at the leading edge whether or not the ROOTS column
/// exists at all: applying a path scope removes the gutter, and the rail does
/// not step inward to fill it — the ROOTS column is *inside* the gutter the rail
/// already led, not beside it. **And the cells** start at
/// `row.left() + GUTTER` in both, so a column that is 5px wide costs zero px of
/// row width and the message column is where it was.
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

    // The same claim with the column *gone*: a path scope is single-root by
    // definition, so the ROOTS column and its gutter disappear. The rail must
    // not step inward to reclaim the space — that would be the column pushing
    // the rail, which is the one arrangement conformance issue 14 rejected.
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
    // The cells, on the same claim: in the multi-root listing they clear the
    // gutter; in the scoped one they start at the row's own edge.
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

/// The key/value list's own geometry, off the paint: for each key, the value
/// that sits to its right on the same line.
struct DetailFact {
    key: PaintedGalley,
    value: PaintedGalley,
}

/// Read one `KEY value` row out of the details pane.
///
/// The value is found by *position* — on the key's own line, to its right —
/// because a value's text is commit-dependent (a name, a timestamp, a hash) and
/// matching by string would compare the wrong occurrence in a frame that paints
/// the same author's name twice (author and committer).
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

/// **The details pane renders author, date and parents as key/value pairs ABOVE
/// the message body, with keys on the label ink and values on the value ink.**
///
/// The claim is geometry first and text second, because "the pane prints an
/// Author row" is satisfied by an author card, a meta grid, a tooltip and a
/// paragraph. What makes this a *key/value list* is that each key has a value on
/// its own line to its right, every key starts at the same x, and all of them
/// sit above the first line of the message body — the three claims a card, a
/// grid and a paragraph each fail differently.
///
/// The two inks are the two roles, asserted as colours: a key is a field label
/// and takes the muted step (`INK_3`, legal on this content surface, never the
/// dim step, because a commit's own facts may not be rendered only at 3.2:1);
/// a value is the thing the key names and takes the primary step. Both are
/// checked for AA against the pane's own surface, so "the muted step" cannot
/// quietly become a failing one.
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

    // The bundle the criterion names: author, date, parents are all here. The
    // committer is the fourth row for the same reason — it is a fact about the
    // commit, and there is nowhere else in this pane for it to live.
    assert_eq!(facts.len(), 4, "four key/value rows");

    // One key column: every key starts at the same x, and every value is to the
    // right of its own key. That is what makes the list a list.
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
        // The inks, as roles.
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

    // The parents value is a hash — the fact the row is named for.
    let parents = facts.last().expect("the parents row");
    assert_eq!(
        parents.value.text,
        short(&seed.c1),
        "the parents row's value is the parent commit's short reference"
    );

    // **Above the message body.** Measured, not asserted by ordering in the
    // source: the pane's own painted geometry. The body is everything under the
    // subject, so its first line is the deepest thing in the pane, and every key
    // must be above it.
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
    // …and in the order the pane promises: author, date, committer, parents,
    // top to bottom.
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

/// **Selecting a commit still populates the details pane, and the empty state
/// still renders when nothing is selected.**
///
/// The second half is the one that matters: reshaping the populated state is
/// exactly the edit that quietly deletes a page-owned empty state, because the
/// early return that draws it lives above every fact row and nothing in the
/// happy path runs it. So it is asserted from a frame with *no* selection, where
/// the pane must say so and must not paint a single key.
#[test]
fn the_details_pane_populates_on_selection_and_still_renders_its_empty_state() {
    let seed = seeded_project();

    // Nothing selected: the page-owned message, and not one key or value.
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

    // Selected: the pane fills in. The selection is driven through a real press
    // on a real row, because "a click still populates the pane" is a different
    // claim from "a state assignment populates the pane".
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

/// **The details pane's fact list is a type of its own, and it is not the commit
/// context menu's `CommitFacts`.**
///
/// The similar name is the hazard: `CommitFacts` answers "may this verb run" —
/// is the repository dirty, is the branch protected, is the commit reachable from
/// the current branch, is the project multi-root, what is the repository called —
/// and not one of those is a fact *about the commit*. Reusing it would have
/// coupled the pane's contents to the menu's item list, so a new verb with a new
/// gate would have added a row to a pane that says what a commit IS.
///
/// Asserted from the source, because a render seam cannot see a type: the
/// details pane builds its rows from a `DetailRow` list over the `Commit` the log
/// already holds, the builder names no `CommitFacts`, and the menu's own use of
/// `CommitFacts` is the only one left.
#[test]
fn the_details_pane_fact_list_is_its_own_type_and_not_the_menus_action_gates() {
    let code = code_without_comments(LOG_SRC);
    assert!(
        code.contains("struct DetailRow"),
        "the details pane's list is a type of its own"
    );
    let builder = fn_code(LOG_SRC, "commit_detail_rows");
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
    // The menu keeps its own, and the log imports it for the menu alone.
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
        fn_code(LOG_SRC, "details_pane").contains("commit_detail_rows"),
        "the details pane reads its rows from the new builder, not from the menu's \
         gates:\n{}",
        fn_code(LOG_SRC, "details_pane")
    );
}
