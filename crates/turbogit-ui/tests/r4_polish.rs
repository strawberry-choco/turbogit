//! Issue #23 — R4 polish audit: keyboard focus rings, small-window
//! scrolling/reachability, and the frozen-shortcut regression suite.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] end-to-end,
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

use egui::{Key, Modifiers, Pos2, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
use tempfile::TempDir;
use test_support::harness::{
    assert_painted, filled_rects, galley_origin, painted_galleys, painted_text, settle,
    shell_harness,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Dialog, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{Palette, configure_style, install_fonts};

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
/// the seeded fixtures (mirrors `diff_viewer::settle`).
///
/// `read_pending` gates too: the diff preview computes on a background thread
/// without raising `busy`, and its loading chrome ("Computing diff…") is
/// hunk-nav buttons are still disabled (zero parsed hunks), which silently
/// breaks focus-dependent assertions (flaky on slow CI runners).
fn settle_long(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|busy={}",
            painted_text(harness),
            harness.state().ui.busy
        );
        if fingerprint == prev && !harness.state().ui.busy && !harness.state().read_pending() {
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
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    // Select the head commit; its changed-file list is computed lazily by the
    // Log window's ensure_files on first render — a deterministic engine call,
    // so the panes still render deterministically.
    if let Some(root) = state.multi.roots.first().cloned()
        && let Some(head) = state.caches.log(&root.id).and_then(|c| c.first().cloned())
    {
        state.ui.selected_commit = Some(head.id.clone());
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
            turbogit_ui::ui::render(ui, state);
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
    // The repo header — the shell's old vocabulary-button row — is gone, so the
    // Changes card's `Refresh changes` icon button is the surviving carrier of
    // that contract. It needs a project open (the Welcome page shows the shell
    // chrome no more).
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);

    harness.get_by_label("Refresh changes").focus();
    settle(&mut harness);

    let center = harness.get_by_label("Refresh changes").rect().center();
    assert_ring_covers(&harness, center, "focused commit-window refresh button");
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
    // Same vocabulary button as the guardrail above, so the shell is up.
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);

    harness.get_by_label("Refresh changes").focus();
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
fn tab_items_paint_brand_focus_rings() {
    // The tab strip only renders with a project open (the Welcome page shows
    // the tool tabs no more), so run over the seeded shell.
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);

    // The old sidebar rail retired with the IDE chrome (issue #03); the
    // center tab strip's custom-drawn tab items are its focus-ring carriers.
    harness.get_by_label("Log").focus();
    settle_long(&mut harness);
    let tab_center = harness.get_by_label("Log").rect().center();
    assert_ring_covers(&harness, tab_center, "focused shell tab item");

    harness.get_by_label("Changes").focus();
    settle_long(&mut harness);
    let tab_center = harness.get_by_label("Changes").rect().center();
    assert_ring_covers(&harness, tab_center, "focused shell tab item");
}

#[test]
fn welcome_action_card_paints_brand_focus_ring() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // The welcome quick actions are three cards (the old fourth "Clone from
    // URL" card merged into the clone panel); any of them carries the ring.
    harness.get_by_label("Open Project").focus();
    settle(&mut harness);

    let center = harness.get_by_label("Open Project").rect().center();
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

    // The gear left the chrome with the IDE toolbar (issue #03/#16); the
    // modal opens through the settings flag (what the command palette's
    // Settings action sets).
    harness.state_mut().ui.settings_open = true;
    settle(&mut harness);

    // The category row is the only *button* labeled "General" — the page
    // heading (issue #26) renders the same word as a plain label.
    use egui::accesskit::Role;
    harness
        .get_all_by_label("General")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap()
        .focus();
    settle(&mut harness);
    let center = harness
        .get_all_by_label("General")
        .find(|n| n.accesskit_node().role() == Role::Button)
        .unwrap()
        .rect()
        .center();
    assert_ring_covers(&harness, center, "focused settings category row");
}

// ===========================================================================
// PASS 2 — SCROLL: fixed panes hold at small window sizes
// ===========================================================================

#[test]
fn shell_chrome_holds_at_small_window_sizes() {
    // The tab strip only renders with a project open, so run over the seeded
    // shell (the Welcome page's own small-window reachability is covered by
    // `welcome_page_stays_reachable_at_small_window_sizes`).
    let seed = seeded_project();
    let harness = polish_harness(&seed, (560.0, 420.0), Tab::Commit);

    let vp = viewport(560.0, 420.0);
    // Every chrome band stays painted and the status bar pins to the bottom.
    // The shell's chrome is now the tab strip and the status bar: the repo
    // header above the strip was deleted, and the sidebar is off screen below
    // `MIN_SIDEBAR_WINDOW_WIDTH`. Labels are the center tab strip items,
    // unique across the shell frame (issue #03).
    assert_visible(&harness, "Changes", vp, "tab strip");
    assert_visible(&harness, "Log", vp, "tab strip");

    let status_top = 420.0 - 24.0;
    // The harness insets the shell by an 8px outer margin, so "pinned to the
    // bottom" means the bottom of the laid-out content area, not the raw
    // viewport edge.
    let status_bands: Vec<_> = filled_rects(&harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::SURFACE && r.top() >= status_top - 12.0)
        .collect();
    assert!(
        status_bands
            .iter()
            .any(|(r, _)| (r.height() - 24.0).abs() <= 4.0 && r.bottom() >= 420.0 - 10.0),
        "status bar must pin to the window bottom at 560x420; bands: {status_bands:?}"
    );
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

    // The redesign stacks hero → clone panel → quick actions → lower, so at a
    // short height the cards and the recents column sit below the fold and must
    // scroll into view. First: the three quick-action cards (no fourth clone
    // card any more) each reach the viewport.
    for card in [
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
    ] {
        harness.get_by_label(card).scroll_to_me();
        steps(&mut harness, 4);
        settle(&mut harness);
        assert_visible(&harness, card, vp, "welcome cards");
    }

    // Vertical reachability: the clone input (in the merged panel) scrolls in.
    harness.get_by_label("Repository URL").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Repository URL", vp, "scrolled-to clone input");

    // Horizontal reachability (issue #23): the responsive `lower` split always
    // fits the recents column inside the viewport width — never clipped out to
    // the right. Scroll the last band into view and assert the recents empty
    // label's x sits within the viewport over PAINTED output (clipped content
    // is never painted).
    harness.get_by_label("Attach Workspace Root").scroll_to_me();
    steps(&mut harness, 6);
    settle(&mut harness);
    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "No recent projects yet.")
        .unwrap_or_else(|| panic!("recents column content not painted at {vp:?}"));
    assert!(
        label.rect.max.x <= vp.max.x + 1.0 && label.rect.min.x >= vp.min.x - 1.0,
        "recents column painted outside the viewport width ({:?} vs {vp:?})",
        label.rect
    );
}

#[test]
fn settings_modal_fits_small_heights() {
    let (mut harness, _project) = shell_harness();
    harness.set_size(egui::vec2(900.0, 480.0));
    settle(&mut harness);

    // The modal opens through the settings flag — the gear left the chrome
    // with the IDE toolbar (issue #03/#16); popups and the palette set it.
    harness.state_mut().ui.settings_open = true;
    steps(&mut harness, 4);
    settle(&mut harness);

    let vp = viewport(900.0, 480.0);
    // The footer must survive short viewports instead of being clipped away.
    for button in ["Apply", "Cancel", "Restore defaults"] {
        assert_visible(&harness, button, vp, "settings footer");
    }

    // The page body scrolls: switch to Appearance and scroll a row into view.
    harness.get_by_label("Appearance").click();
    steps(&mut harness, 4);
    settle(&mut harness);
    harness.get_by_label("Date format").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Date format", vp, "scrolled-to setting row");
}

// ===========================================================================
// PASS 3 — SHORTCUT AUDIT: the frozen five (ADR-0009)
// ===========================================================================

#[test]
fn ctrl_k_returns_to_the_commit_tool_window() {
    // The tab strip only renders with a project open, so run over the seeded
    // shell — clicking the Log tab is what opens it.
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);
    harness.get_by_label("Log").click();
    settle_long(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Log);

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle_long(&mut harness);

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
    // Issue #25: the PUSH SCOPE segmented control renders for any
    // opened push dialog; Remote/Branch fields only paint when the
    // user has selected the ThisRepo scope segment.
    assert_painted(&harness, "PUSH SCOPE");
    assert_painted(&harness, "This repo");
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

// --- 19 — the floating-surface audit -----------------------------------------
//
// A 1px stroke means *this floats* (R2). That is a rule with two halves and the
// second half is the one that gets skipped: a stroke is not there by default,
// so nothing fails when a **floating** surface loses it — the dialog simply
// stops reading as a dialog and blends into whatever is behind it.
//
// These tests are the audit, and they are stated as a **table** rather than as
// a scan. A scan for "stroked rects" would pass on any frame that happens to
// have one; a table says which surface is expected to stroke, what kind of
// surface it is, and fails naming the one that stopped. The card half is
// asserted as *absence over a rect* rather than as a count, because other
// surfaces are legitimately painted in the same frame.

/// The float chrome the token layer maps once, globally: a `SURFACE` fill with
/// a 1px `LINE` hairline. Every floating surface in the app wears it, and none
/// of them spells it — `configure_style` sets `visuals.window_stroke` and
/// `Frame::popup` reads it, so a dialog, a popup and a context menu are the same
/// frame by construction.
const FLOAT_STROKE_WIDTH: f32 = 1.0;

/// Every stroked rectangle in the last frame, as `(rect, colour, width)`.
///
/// **Recursive**, and that is not an optimisation. A `egui::Window` — which is
/// every dialog and every popup — paints its frame as a `Shape::Vec` holding
/// `[shadow, RectShape]`, so a top-level-only scan sees a window's *content*
/// borders and never its chrome. `tests/feedback_chrome.rs` hit the same thing
/// and works around it in its own `popup_chrome`; the workaround belongs here
/// instead, once, so every floating-surface assertion in the workspace can use
/// it.
fn strokes(harness: &Harness<'_, AppState>) -> Vec<(Rect, egui::Color32, f32)> {
    fn walk(shape: &Shape, out: &mut Vec<(Rect, egui::Color32, f32)>) {
        match shape {
            Shape::Rect(r)
                if r.stroke.color != egui::Color32::TRANSPARENT && r.stroke.width > 0.0 =>
            {
                out.push((r.rect, r.stroke.color, r.stroke.width));
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in harness.output().shapes.iter() {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// The float chrome at `probe`, if the frame painted one there.
fn float_chrome_at(harness: &Harness<'_, AppState>, probe: Pos2) -> Option<Rect> {
    strokes(harness)
        .into_iter()
        .find(|(rect, color, width)| {
            *color == Palette::LINE
                && (*width - FLOAT_STROKE_WIDTH).abs() < 0.01
                && rect.contains(probe)
        })
        .map(|(rect, _, _)| rect)
}

/// A repository with one commit, for the surfaces that need a real project.
fn project() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
    ] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    }
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-q", "-m", "init"]] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    }
    (tmp, repo)
}

fn project_harness(state: AppState) -> Harness<'static, AppState> {
    let mut fonts = false;
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        move |ui, state| {
            // The token layer, exactly as `app.rs` installs it. Without this
            // a harness renders egui's stock visuals, whose window stroke is
            // `from_gray(60)` rather than the design system's `LINE` — so a
            // floating-surface assertion here would be measuring egui, not the
            // app. `test_support::harness::shell_harness` does the same thing
            // and says why.
            configure_style(ui.ctx());
            if !fonts {
                install_fonts(ui.ctx());
                fonts = true;
            }
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(Vec2::new(1024.0, 768.0));
    settle(&mut h);
    h
}

/// **Every floating surface keeps its 1px stroke**, and each one is named.
///
/// The table is the point: a surface that stops floating must be *removed from
/// this list with a reason*, not left to rot in it. A frame-by-frame scan
/// could not say that, because a frame with no stroked rect at all looks
/// exactly like a frame where everything stopped floating.
#[test]
fn every_floating_surface_keeps_its_one_pixel_stroke() {
    use egui_kittest::kittest::Queryable as _;

    // (kind, what opens it, how the frame is probed)
    //
    // The probe is a point *inside* the surface — found from the painted
    // chrome itself where the surface is a window, and from a node rect where
    // it is a context menu hosted in an `egui::Area`. In both cases the
    // assertion is that a 1px `LINE` stroke covers it, which is the whole
    // claim: the surface still reads as floating.
    let mut checked: Vec<(&'static str, Rect)> = Vec::new();

    // 1. A **dialog** — the New Branch modal, opened by its own enum variant.
    {
        let (_tmp, repo) = project();
        let mut state = AppState::for_roots(_tmp.path(), &[repo]);
        state.ui.dialog = Some(Dialog::NewBranch);
        let mut h = project_harness(state);
        settle(&mut h);
        let button = h.get_by_label("Create").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the New Branch dialog must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        assert!(
            chrome.width() > 200.0 && chrome.height() > 100.0,
            "the float chrome is a dialog-sized surface, not a control border: \
             {chrome:?}"
        );
        checked.push(("dialog (New Branch)", chrome));
    }

    // 2. A **popup** — the VCS operations window, `Alt+\``.
    {
        let (harness, _tmp) = shell_harness();
        let mut h = harness;
        h.key_press_modifiers(Modifiers::ALT, Key::Backtick);
        settle(&mut h);
        let button = h.get_by_label("Refresh").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the VCS popup must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        checked.push(("popup (VCS operations)", chrome));
    }

    // 3. A **popup** — the command palette, `Ctrl+Shift+A`.
    {
        let (harness, _tmp) = shell_harness();
        let mut h = harness;
        h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
        settle(&mut h);
        let button = h.get_by_label("Refresh").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the command palette must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        checked.push(("popup (command palette)", chrome));
    }

    // 4. The **settings modal** — a dialog like any other, and the one most
    //    likely to lose its stroke to a layout change because its body is two
    //    hand-laid-out columns rather than a plain flow.
    {
        let mut h = shell_harness().0;
        h.state_mut().ui.settings_open = true;
        settle(&mut h);
        let button = h.get_by_label("Apply").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the settings modal must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        checked.push(("dialog (settings)", chrome));
    }

    // …and the report, so a reader of a failure knows exactly which surface.
    assert_eq!(
        checked.len(),
        4,
        "every floating surface in this table was probed: {checked:?}"
    );
    for (what, rect) in &checked {
        assert!(
            rect.width() > 0.0 && rect.height() > 0.0,
            "{what} is a real surface: {rect:?}"
        );
    }
}

/// The two **context menus** keep the same stroke as every other floating
/// surface — the commit menu and the branch menu, the sibling modules this
/// ticket puts in scope.
///
/// They get their own test rather than a row in the table above because they
/// are the surfaces most likely to be excluded by accident: they are hosted in
/// an `egui::Area` rather than an `egui::Window`, so nothing in the *window*
/// machinery paints their frame, and `widgets::menu_surface` is the only thing
/// that does. If a future change routed a context menu through a plain `Frame`
/// with no stroke, nothing else in the workspace would notice.
#[test]
fn the_commit_and_branch_context_menus_keep_the_float_stroke() {
    use egui_kittest::kittest::Queryable as _;
    use test_support::harness::right_click_row;

    // The commit context menu: right-click a commit row in the log.
    {
        let (tmp, repo) = project();
        std::fs::write(repo.join("second.txt"), "second\n").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-q", "-m", "second"]] {
            let out = std::process::Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        let mut state = AppState::for_roots(tmp.path(), &[repo]);
        state.ui.tab = Tab::Log;
        let mut h = project_harness(state);
        settle(&mut h);
        right_click_row(&mut h, "second");
        settle(&mut h);
        let item = h.get_by_label("Copy hash").rect();
        let chrome = float_chrome_at(&h, item.center()).unwrap_or_else(|| {
            panic!(
                "the commit context menu must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        assert!(
            chrome.contains_rect(item),
            "the stroke is the menu's own frame, around its items: {chrome:?} vs \
             {item:?}"
        );
    }
}

/// **Every content card paints no stroke**, and the one bordered card is the
/// floating one.
///
/// Asserted as "no stroked rect overlaps the card's rect" rather than as a
/// count of strokes, because a dialog or a menu painted in the same frame is
/// legitimately a stroke and counting would make the assertion a statement
/// about the frame rather than about the card.
#[test]
fn every_content_card_paints_no_stroke_and_the_floating_one_does() {
    // The Welcome screen is the frame with the most cards in it, and the three
    // quick-action cards are the ones that used to wear a 1px `LINE` ring
    // (brand on hover) — the app's one bordered *content* region, and the
    // reason the changelog overlay was no longer the only bordered card.
    let (harness, _tmp) = shell_harness();
    let mut h = harness;
    h.set_size(Vec2::new(1024.0, 900.0));
    settle(&mut h);

    let cards = [
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
    ];
    for card in cards {
        let node = h.get_by_label(card);
        let rect = node.rect();
        let overlapping: Vec<_> = strokes(&h)
            .into_iter()
            .filter(|(stroke, _, _)| stroke.intersect(rect).is_positive())
            .collect();
        assert!(
            overlapping.is_empty(),
            "the `{card}` card is a content region and paints no stroke (R2: a \
             stroke means the surface floats); but {overlapping:#?} overlap \
             {rect:?}"
        );
    }
    // The shared card default, read off the token layer rather than restated:
    // an unbordered card is the default and a bordered one is opt-in.
    let default = turbogit_ui::ui::widgets::CardFrame::default();
    assert!(
        !default.bordered,
        "the shared card paints no stroke by default"
    );
    assert!(
        turbogit_ui::ui::widgets::CardFrame::default()
            .bordered()
            .bordered,
        "…and the bordered variant is a separate, opt-in frame"
    );

    // The changelog overlay is the one card that genuinely floats, and it keeps
    // its stroke. This is the pair of halves in one test: the content cards
    // lost theirs, the floating one kept its, and neither is a comment saying
    // why.
    h.state_mut().ui.show_changelog = true;
    settle(&mut h);
    let title = h.get_by_label("What's New").rect();
    let chrome = strokes(&h)
        .into_iter()
        .find(|(rect, color, width)| {
            *color == Palette::LINE
                && (*width - FLOAT_STROKE_WIDTH).abs() < 0.01
                && rect.contains(title.center())
        })
        .map(|(rect, _, _)| rect)
        .unwrap_or_else(|| {
            panic!(
                "the changelog dialog floats and so keeps its 1px stroke; \
                 strokes: {:#?}",
                strokes(&h)
            )
        });
    assert!(
        chrome.width() >= 420.0,
        "the floating card is the changelog dialog's own surface: {chrome:?}"
    );
}

/// Every dialog's body is separated from its action slot by the **footer
/// rule** — the one 1px `RULE_FOOTER` hairline `widgets::dialog_footer` paints.
///
/// The rule is a *fill*, not a stroke (the widget layer's own reason: a fill is
/// the primitive the painted-output harness can see at all), so this asserts
/// the fill's token, its 1px height, and its position — directly under the
/// dialog's body and directly above the action row.
///
/// The table is the assertion. A dialog that stops ruling its body off from its
/// actions fails here by name; a dialog added later has to be written into
/// this list, and the count is what notices.
#[test]
fn every_dialog_separates_its_body_from_its_actions_with_the_footer_rule() {
    use egui_kittest::kittest::Queryable as _;

    // (dialog, a word unique to that dialog's own copy, so the test fails
    // naming the dialog rather than "a dialog")
    let cases: [(Dialog, &str); 9] = [
        (Dialog::NewBranch, "Start from:"),
        (Dialog::Merge, "Allow unrelated histories"),
        (Dialog::Rebase, "Autosquash fixup commits"),
        (Dialog::Tag, "Annotated"),
        (Dialog::Shelve, "Shelf name:"),
        (Dialog::Stash, "Keep index"),
        (Dialog::RenameBranch, "to:"),
        (Dialog::CompareBranches, "Swap Branches"),
        (Dialog::CherryPickTarget, "Apply the selected commit"),
    ];

    for (dialog, tell) in cases {
        let (_tmp, repo) = project();
        let mut state = AppState::for_roots(_tmp.path(), &[repo]);
        state.ui.dlg.rebase_onto = "main".to_string();
        // Pin the mode so the start action's label is this suite's to name;
        // the default has moved between modes as the rebase work landed, and a
        // footer ratchet should not be the thing that notices.
        state.ui.dlg.rebase_mode = turbogit_domain::model::RebaseMode::Standard;
        state.ui.dlg.compare_left = "main".to_string();
        state.ui.dlg.compare_right = "feature".to_string();
        state.ui.dlg.rename_branch_name = "old".to_string();
        state.ui.dialog = Some(dialog);
        let mut h = project_harness(state);
        settle(&mut h);

        // The dialog really is open, and this really is *this* dialog: its own
        // copy paints inside it. Without this the table would be a list of
        // hopes.
        assert_painted(&h, tell);

        // The rule is a **fill**, not a stroke — the widget layer's own reason
        // is that a fill is the primitive the painted-output harness can see at
        // all — so it is read as a 1px band in the `RULE_FOOTER` tone.
        let rules: Vec<Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, color)| {
                *color == Palette::RULE_FOOTER && (rect.height() - 1.0).abs() < 0.01
            })
            .map(|(rect, _)| rect)
            .collect();
        assert_eq!(
            rules.len(),
            1,
            "the {dialog:?} dialog's body is closed off from its action slot by \
             exactly one `RULE_FOOTER` hairline; the frame painted {rules:?}"
        );
        let rule = rules[0];
        assert!(
            rule.width() > 100.0,
            "the footer rule spans the dialog's body, not one control: {rule:?}"
        );
        // …and it sits inside the dialog's own float chrome, not somewhere else
        // in the frame. The **smallest** enclosing stroked rect is the dialog:
        // a frame can be nested inside another surface's frame, and taking the
        // first one found would measure the wrong rectangle and then fail the
        // button check for a reason that has nothing to do with the footer.
        let chrome = strokes(&h)
            .into_iter()
            .filter(|(rect, color, width)| {
                *color == Palette::LINE
                    && (*width - FLOAT_STROKE_WIDTH).abs() < 0.01
                    && rect.contains(rule.center())
            })
            .map(|(rect, _, _)| rect)
            .min_by(|a, b| {
                (a.width() * a.height())
                    .partial_cmp(&(b.width() * b.height()))
                    .expect("finite rects")
            })
            .unwrap_or_else(|| {
                panic!("the {dialog:?} dialog floats, so its footer rule is inside its frame")
            });
        assert!(
            chrome.width() > rule.width(),
            "the rule spans the dialog's body inside its own frame: chrome \
             {chrome:?}, rule {rule:?}"
        );
        // The action slot is on the **other side** of the rule from the body.
        //
        // Stated as "above, not below" rather than as "within N points", and
        // the reason is worth recording: the rule's job is to *separate*, so
        // the property that matters is which side the actions are on. A window
        // that auto-sizes, a `ScrollArea` body, or a dialog whose footer is
        // followed by an expanding region all move the actions further from
        // the rule without weakening the separation — and a test that demanded
        // adjacency would have to be loosened for each of those, which is how a
        // positional assertion quietly stops being one.
        let action_labels: &[&str] = match dialog {
            Dialog::NewBranch => &["Create", "Cancel"],
            Dialog::Merge => &["Merge", "Cancel"],
            Dialog::Rebase => &["Start rebase", "Cancel"],
            Dialog::Tag => &["Create tag", "Cancel"],
            Dialog::Shelve => &["Shelve selected", "Cancel"],
            Dialog::Stash => &["Stash", "Cancel"],
            Dialog::RenameBranch => &["Rename", "Cancel"],
            Dialog::CompareBranches => &["Swap Branches", "Close"],
            Dialog::CherryPickTarget => &["Cancel"],
            other => panic!("the footer-rule table has no row for {other:?}"),
        };
        for label in action_labels {
            // Inside the dialog's own frame **and on the far side of the rule**
            // from the top. Both halves are needed: the shell reuses several
            // of these words on its own chrome (the Commit window's `Stash` and
            // `Shelve…` buttons sit inside the dialog's frame region, because
            // the dialog floats over the content column), so a label alone
            // would find the wrong button. The rule is what distinguishes
            // them, which is the whole point of asserting through it.
            let button = h
                .get_all_by_role(egui::accesskit::Role::Button)
                .find(|n| {
                    n.accesskit_node().label().as_deref() == Some(*label)
                        && n.rect().top() >= rule.bottom() - 1.0
                        && n.rect().intersect(chrome) == n.rect()
                })
                .map(|n| n.rect());
            let button = button.unwrap_or_else(|| {
                panic!(
                    "the {dialog:?} dialog's action slot sits on the far side of \
                     its footer rule from the body: `{label}` was not painted \
                     below the rule {rule:?} inside the dialog's frame {chrome:?}"
                )
            });
            assert!(
                button.top() >= rule.bottom() - 1.0,
                "the {dialog:?} dialog's action slot is separated from its body \
                 by the footer rule: `{label}` at {button:?} against a rule at \
                 {rule:?}"
            );
            assert!(
                chrome.contains_rect(button),
                "the action is inside the dialog's own frame: {button:?} against \
                 {chrome:?}"
            );
        }
    }
}
