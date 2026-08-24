//! Issue #23 — R4 polish audit: keyboard focus rings, small-window
//! scrolling/reachability, and the frozen-shortcut regression suite.
//!
//! Headless egui_kittest harness driving [`turbogit::ui::render`] end-to-end,
//! asserting only on public surfaces:
//!
//! - **Painted output** — BRAND focus-ring strokes (token spec §7.2: a 1px
//!   BRAND ring marks whatever holds keyboard focus), text galleys and their
//!   positions for reachability at small window sizes.
//! - **Accessibility tree** — node rects and focus actions via kittest's
//!   `Queryable` (the same surface screen readers use).
//! - **State transitions** — public `AppState` fields after the frames.
//!
//! Covered (spec §7.2 focus rings, §R4.4 keyboard audit, ADR-0009):
//! - every interactive surface paints exactly one BRAND ring while focused
//! - fixed panes keep their minimum sizes and stay reachable at small
//!   harness window sizes (scrolling brings below-fold content into view)
//! - the five frozen shortcuts dispatch unchanged, fire even when a text
//!   field holds focus, and never collide with plain typing or each other

mod common;

use common::{assert_painted, galley_origin, settle, shell_harness};
use egui::{Key, Modifiers, Pos2, Rect, Shape, Vec2};
use egui_kittest::{kittest::Queryable, Harness};
use tempfile::TempDir;
use turbogit::engine::cli::CliExecutor;
use turbogit::engine::{AppEvent, GitExecutor};
use turbogit::model::{LogOpts, VcsSettings};
use turbogit::state::{AppState, Dialog, Tab};
use turbogit::theme::{configure_style, install_fonts, Palette};

// --- Shared helpers -----------------------------------------------------------

#[derive(Debug, PartialEq)]
struct ModalState {
    tab: Tab,
    dialog: Option<Dialog>,
    vcs_popup: bool,
    command_palette: bool,
    branches_popup: bool,
    settings_open: bool,
}

fn modal_state(harness: &Harness<'_, AppState>) -> ModalState {
    let s = harness.state();
    ModalState {
        tab: s.ui.tab,
        dialog: s.ui.dialog,
        vcs_popup: s.ui.vcs_popup,
        command_palette: s.ui.command_palette,
        branches_popup: s.ui.branches_popup,
        settings_open: s.ui.settings_open,
    }
}

/// Rects of every BRAND stroke painted by the last frame — the token-spec
/// focus rings (§7.2). Regular borders are LINE-colored fills or strokes, so
/// a BRAND stroke is unambiguous: only focus rings paint one.
fn brand_rings(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) if rs.stroke.width >= 1.0 && rs.stroke.color == Palette::BRAND => {
                Some(rs.rect)
            }
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_ring_covers(harness: &Harness<'_, AppState>, point: Pos2, what: &str) {
    let rings = brand_rings(harness);
    assert!(
        rings.iter().any(|r| r.contains(point)),
        "{what}: no BRAND focus ring covers {point:?}; rings painted: {rings:?}"
    );
}

fn viewport(w: f32, h: f32) -> Rect {
    Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h))
}

#[track_caller]
fn assert_visible(harness: &Harness<'_, AppState>, label: &str, vp: Rect, what: &str) -> Rect {
    let rect = harness.get_by_label(label).rect();
    assert!(
        vp.intersects(rect),
        "{what}: `{label}` sits outside the viewport (rect {rect:?}, viewport {vp:?})"
    );
    rect
}

/// Step frames until painted output AND async engine activity stabilize.
/// Budgeted by wall-clock time so a contended `git` subprocess cannot starve
/// the seeded fixtures (mirrors `redesign_diff::settle`).
fn settle_long(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|busy={}",
            common::painted_text(harness),
            harness.state().ui.busy
        );
        if fingerprint == prev && !harness.state().ui.busy {
            return;
        }
        prev = fingerprint;
    }
    panic!("layout did not settle within 15s");
}

/// Run exactly `n` frames. Scroll actions (`scroll_to_me`, `scroll_*`) are
/// applied by egui on the pass AFTER the one consuming the request, so a
/// text-fingerprint settle can legitimately exit before the scroll lands —
/// callers follow scroll actions with a few fixed steps instead.
fn steps(harness: &mut Harness<'_, AppState>, n: usize) {
    for _ in 0..n {
        harness.step();
    }
}

// --- Seeded single-root fixture -------------------------------------------------

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    /// HEAD~1 — "alpha: initial commit".
    c1: String,
    /// HEAD — "alpha: second commit" (row label carrier).
    c2: String,
}

use std::path::{Path, PathBuf};

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
    std::fs::write(dir.join(name), msg).expect("writing work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", msg]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");

    run_git(&alpha, &["init", "-b", "main"]);
    run_git(&alpha, &["config", "user.email", "test@example.com"]);
    run_git(&alpha, &["config", "user.name", "Test"]);
    let c1 = commit_file(&alpha, "file.txt", "alpha: initial commit");
    let c2 = commit_file(&alpha, "file.txt", "alpha: second commit");

    // An unstaged working-tree edit so the diff preview (Local comparison)
    // has real content to render.
    std::fs::write(alpha.join("file.txt"), "alpha: second commit\nlocal edit\n")
        .expect("working-tree edit");

    Seed {
        _tmp: tmp,
        project,
        alpha,
        c1,
        c2,
    }
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Harness over the seeded project with deterministic caches (the production
/// app fills them asynchronously; priming through the production event path
/// keeps these tests off wall-clock timing except where real git work is
/// unavoidable). Drains the worker-event channel every frame like `app.rs`
/// does.
fn polish_harness(seed: &Seed, size: (f32, f32), tab: Tab) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert!(
        !state.multi.roots.is_empty(),
        "seeded repo root must be discovered"
    );
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    // Prime the log cache through the production event path (decision 9):
    // AppEvent::LogLoaded via state.tx + drain_events().
    for root in state.multi.roots.clone() {
        let commits = engine
            .log(&root.path, &LogOpts::default())
            .expect("seeded log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    // Select the head commit; its changed-file list is computed lazily by the
    // Log window's ensure_files on first render — a deterministic engine call,
    // so the panes still render deterministically.
    if let Some(root) = state.multi.roots.first().cloned() {
        if let Some(head) = state.caches.log(&root.id).and_then(|c| c.first().cloned()) {
            state.ui.selected_commit = Some(head.id.clone());
        }
    }
    state.ui.tab = tab;

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events(); // production parity with app.rs
            turbogit::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(size.0, size.1));
    settle_long(&mut harness);
    harness
}

// ===========================================================================
// PASS 1 — FOCUS: visible BRAND rings per token spec §7.2
// ===========================================================================

// --- Vocabulary widgets (already compliant — regression guardrails) --------

#[test]
fn vocabulary_button_paints_brand_focus_ring_when_focused() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Update Project").focus();
    settle(&mut harness);

    let center = harness.get_by_label("Update Project").rect().center();
    assert_ring_covers(&harness, center, "focused toolbar button");
}

#[test]
fn text_input_paints_brand_focus_ring_when_focused() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Repository URL").focus();
    settle(&mut harness);

    let center = harness.get_by_label("Repository URL").rect().center();
    assert_ring_covers(&harness, center, "focused clone URL input");
}

/// Exactly ONE ring may be visible at any time — keyboard focus must never
/// be ambiguous about which widget it sits on (§R4.4).
#[test]
fn only_one_focus_ring_is_visible_at_a_time() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Update Project").focus();
    settle(&mut harness);

    let rings = brand_rings(&harness);
    assert_eq!(
        rings.len(),
        1,
        "exactly one focus ring may be painted; got {rings:?}"
    );
}

// --- Custom-drawn controls (this pass fixes them) ----------------------------

#[test]
fn rail_and_tab_widgets_paint_brand_focus_rings() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Git Log").focus();
    settle(&mut harness);
    let rail_center = harness.get_by_label("Git Log").rect().center();
    assert_ring_covers(&harness, rail_center, "focused sidebar rail button");

    harness.get_by_label("Log").focus();
    settle(&mut harness);
    let tab_center = harness.get_by_label("Log").rect().center();
    assert_ring_covers(&harness, tab_center, "focused shell tab item");
}

#[test]
fn welcome_action_card_paints_brand_focus_ring() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Clone from URL").focus();
    settle(&mut harness);

    let center = harness.get_by_label("Clone from URL").rect().center();
    assert_ring_covers(&harness, center, "focused welcome action card");
}

#[test]
fn log_rows_paint_brand_focus_rings() {
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Log);

    // Commit-table rows — newest first, then the older one.
    let row_label = format!("{} {}", short(&seed.c2), "alpha: second commit");
    harness.get_by_label(&row_label).focus();
    settle_long(&mut harness);
    let center = harness.get_by_label(&row_label).rect().center();
    assert_ring_covers(&harness, center, "focused commit row");

    let older_label = format!("{} {}", short(&seed.c1), "alpha: initial commit");
    harness.get_by_label(&older_label).focus();
    settle_long(&mut harness);
    let center = harness.get_by_label(&older_label).rect().center();
    assert_ring_covers(&harness, center, "focused older commit row");

    // Changed-file row (visible because the head commit is preselected).
    harness.get_by_label("file.txt").focus();
    settle_long(&mut harness);
    let center = harness.get_by_label("file.txt").rect().center();
    assert_ring_covers(&harness, center, "focused changed-file row");

    // Roots-filter row in the branches pane.
    harness.get_by_label("All roots").focus();
    settle_long(&mut harness);
    let center = harness.get_by_label("All roots").rect().center();
    assert_ring_covers(&harness, center, "focused roots-filter row");
}

#[test]
fn diff_toolbar_controls_paint_brand_focus_rings() {
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);

    // Open the inline preview (same transition clicking a change row does).
    harness.state_mut().ui.preview_change = Some(seed.alpha.join("file.txt"));
    settle_long(&mut harness);
    assert_painted(&harness, "Side-by-Side");

    for label in ["Unified", "Repo", "Next hunk"] {
        harness.get_by_label(label).focus();
        settle_long(&mut harness);
        let center = harness.get_by_label(label).rect().center();
        assert_ring_covers(&harness, center, &format!("focused diff control {label}"));
    }
}

#[test]
fn settings_category_row_paints_brand_focus_ring() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("settings").click();
    settle(&mut harness);

    harness.get_by_label("Version Control").focus();
    settle(&mut harness);
    let center = harness.get_by_label("Version Control").rect().center();
    assert_ring_covers(&harness, center, "focused settings category row");
}

// ===========================================================================
// PASS 2 — SCROLL: fixed panes hold at small window sizes
// ===========================================================================

#[test]
fn shell_chrome_holds_at_small_window_sizes() {
    let (mut harness, _project) = shell_harness();
    harness.set_size(egui::vec2(560.0, 420.0));
    settle(&mut harness);

    let vp = viewport(560.0, 420.0);
    // Every chrome band stays painted and the status bar pins to the bottom.
    // Labels are chosen unique across the shell ("Commit" exists three times:
    // toolbar button, rail button, tab item).
    assert_visible(&harness, "File", vp, "topbar");
    assert_visible(&harness, "Update Project", vp, "toolbar");
    assert_visible(&harness, "Git Log", vp, "sidebar rail");
    assert_visible(&harness, "Log", vp, "tab strip");

    let status_top = 420.0 - 24.0;
    // The harness insets the shell by an 8px outer margin, so "pinned to the
    // bottom" means the bottom of the laid-out content area, not the raw
    // viewport edge.
    let status_bands: Vec<_> = common::filled_rects(&harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::SURFACE && r.top() >= status_top - 12.0)
        .collect();
    assert!(
        status_bands
            .iter()
            .any(|(r, _)| (r.height() - 24.0).abs() <= 4.0 && r.bottom() >= 420.0 - 10.0),
        "status bar must pin to the window bottom at 560x420; bands: {status_bands:?}"
    );

    // Welcome body remains scrollable rather than truncated: the clone form
    // below the fold can be scrolled into view.
    harness.get_by_label("Repository URL").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Repository URL", vp, "scrolled-to clone input");
}

#[test]
fn log_panes_stay_reachable_at_small_window_sizes() {
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Log);
    let row_label = format!("{} {}", short(&seed.c2), "alpha: second commit");

    for (w, h) in [(720.0f32, 480.0f32), (600.0, 400.0)] {
        harness.set_size(egui::vec2(w, h));
        settle_long(&mut harness);
        let vp = viewport(w, h);
        let what = &format!("at {w}x{h}");

        // Pane 1 (branches) keeps a usable search input at its 140px floor;
        // pane 2 (graph) keeps its toolbar and newest commit row; panes 3+4
        // keep both headers.
        let search = assert_visible(&harness, "Search branches", vp, what);
        assert!(
            search.width() >= 60.0,
            "{what}: branches pane collapsed (search input {search:?})"
        );
        assert_visible(&harness, "Search commits", vp, what);
        assert_visible(&harness, &row_label, vp, what);
        assert_visible(&harness, "COMMIT DETAILS", vp, what);
        assert_painted(&harness, "CHANGED FILES");

        // The graph pane must retain positive width: the hash cell of the
        // newest row is painted inside the viewport, not clipped away.
        let hash_pos = galley_origin(&harness, &short(&seed.c2))
            .unwrap_or_else(|| panic!("{what}: newest commit hash not painted"));
        assert!(
            vp.contains(hash_pos),
            "{what}: commit hash painted at {hash_pos:?} outside {vp:?} — graph pane collapsed"
        );
    }
}

#[test]
fn welcome_page_stays_reachable_at_small_window_sizes() {
    let (mut harness, _project) = shell_harness();
    harness.set_size(egui::vec2(520.0, 440.0));
    settle(&mut harness);

    let vp = viewport(520.0, 440.0);
    for card in ["Clone from URL", "Open Project", "Initialize Repository"] {
        assert_visible(&harness, card, vp, "welcome cards");
    }

    // Vertical reachability: the clone form sits below the fold and scrolls
    // into view.
    harness.get_by_label("Repository URL").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Repository URL", vp, "scrolled-to clone input");

    // Horizontal reachability: the responsive grid guarantees the recents
    // column fits the viewport by construction — nothing clips out of
    // reach. Asserted over PAINTED output (clipped content is never
    // painted), since accesskit bounding boxes of fully-clipped nodes are
    // unreliable.
    // Horizontal reachability: the responsive grid guarantees the recents
    // column fits the viewport by construction — nothing clips out of
    // reach horizontally. Asserted over PAINTED output (clipped content is
    // never painted); the empty-recents label shares the column's x origin
    // with its title, which may legally rest just above the fold after the
    // scroll above.
    let label_pos = galley_origin(&harness, "No recent projects yet.")
        .unwrap_or_else(|| panic!("recents column content not painted — clipped away at {vp:?}"));
    assert!(
        vp.contains(label_pos),
        "recents column painted outside the viewport ({label_pos:?} vs {vp:?})"
    );
}

#[test]
fn settings_modal_fits_small_heights() {
    let (mut harness, _project) = shell_harness();
    harness.set_size(egui::vec2(900.0, 480.0));
    settle(&mut harness);

    // The gear stays pinned to the toolbar's right edge at every window
    // size, so it opens directly.
    harness.get_by_label("settings").click();
    steps(&mut harness, 4);
    settle(&mut harness);

    let vp = viewport(900.0, 480.0);
    // The footer must survive short viewports instead of being clipped away.
    for button in ["Apply", "Cancel", "Reset"] {
        assert_visible(&harness, button, vp, "settings footer");
    }

    // The page body scrolls: content below the fold comes into view.
    harness.get_by_label("Date format:").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Date format:", vp, "scrolled-to setting row");
}

// ===========================================================================
// PASS 3 — SHORTCUT AUDIT: the frozen five (ADR-0009)
// ===========================================================================

#[test]
fn ctrl_k_returns_to_the_commit_tool_window() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    harness.get_by_label("Git Log").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Log);

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);

    assert_eq!(harness.state().ui.tab, Tab::Commit);
}

#[test]
fn ctrl_shift_k_opens_the_push_dialog() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    assert_eq!(harness.state().ui.dialog, None);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    settle(&mut harness);

    assert_eq!(harness.state().ui.dialog, Some(Dialog::Push));
    assert_painted(&harness, "Remote:");
    assert_painted(&harness, "Force push (--force-with-lease)");
}

#[test]
fn ctrl_t_rescans_without_disturbing_shell_state() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(&mut harness);

    let after = modal_state(&harness);
    assert_eq!(after.tab, Tab::Commit);
    assert_eq!(after.dialog, None);
    assert!(!after.command_palette && !after.vcs_popup && !after.branches_popup);
}

#[test]
fn ctrl_shift_a_opens_the_command_palette() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);

    assert!(harness.state().ui.command_palette);
    assert_painted(&harness, "Find Action");
}

#[test]
fn alt_backtick_opens_the_vcs_operations_popup() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::ALT, Key::Backtick);
    settle(&mut harness);

    assert!(harness.state().ui.vcs_popup);
    assert_painted(&harness, "VCS Operations");
}

/// Dispatch-first contract: the frozen five read raw input before any widget,
/// so they must still fire while a text field holds keyboard focus — and the
/// palette must stay open afterwards (no new close-on-shortcut behavior).
#[test]
fn frozen_shortcuts_fire_while_a_text_field_has_focus() {
    // While the Welcome clone URL input is focused…
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    harness.get_by_label("Repository URL").focus();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Commit);

    // …and while the command palette's query field is focused.
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);
    let s = harness.state();
    assert_eq!(s.ui.tab, Tab::Commit);
    assert!(s.ui.command_palette, "palette must stay open across Ctrl+K");
}

/// Plain keys without modifiers never trigger any of the frozen five —
/// typing into inputs must be safe from accidental grabs.
#[test]
fn unmodified_keys_never_trigger_frozen_shortcuts() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    let before = modal_state(&harness);

    for key in [Key::K, Key::T, Key::A, Key::Backtick] {
        harness.key_press(key);
    }
    settle(&mut harness);

    assert_eq!(
        modal_state(&harness),
        before,
        "unmodified keys must not dispatch frozen shortcuts"
    );
}

/// No conflicts between the frozen five themselves: opening the VCS popup
/// does not swallow Ctrl+Shift+K, and both surfaces coexist.
#[test]
fn alt_backtick_and_ctrl_shift_k_coexist_without_conflict() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::ALT, Key::Backtick);
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    settle(&mut harness);

    let s = harness.state();
    assert!(s.ui.vcs_popup, "VCS popup must remain open");
    assert_eq!(s.ui.dialog, Some(Dialog::Push));
}
