use egui::{Color32, Key, Modifiers, Rect};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_galleys, painted_ink,
    settle, shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_ui::theme::{Palette, RAIL_WIDTH, RepoState};
use turbogit_ui::ui::shell;
use turbogit_ui::ui::widgets::TAB_STRIP_HEIGHT;

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    AppState::for_roots(project_dir, roots)
}

/// The narrowest harness width at which the **sidebar** actually renders.
///
/// `shell::MIN_SIDEBAR_WINDOW_WIDTH` is 1000, but the shell measures its work
/// rect *inside* the window frame, so the outer width that clears the
/// threshold is higher than the constant. The shell's central panel no longer
/// carries an inner margin (the frame is flush with the window edge), so the
/// harness's only inset is kittest's own outer panel: measured on the real
/// render, the rail first paints at 1016 = 1000 + 16. A suite that assumed a
/// width without measuring would have asserted against nothing, which is the
/// vacuous-pass trap this ticket's criterion names, and why the number is
/// measured here rather than reasoned about.
pub const SIDEBAR_VISIBLE_FROM: f32 = 1016.0;

const HARNESS_DEFAULT: (f32, f32) = (1024.0, 768.0);

fn rail_visible_size() -> (f32, f32) {
    (
        HARNESS_DEFAULT.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        HARNESS_DEFAULT.1,
    )
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    harness_at(state, HARNESS_DEFAULT.0, HARNESS_DEFAULT.1)
}

/// The same harness at an explicit size.
///
/// **Sized, not defaulted.** `egui_kittest::Harness::builder()` lands on
/// 800×600, which is *below* the sidebar's own threshold, so a suite that
/// never says what size it is renders a shell with no left rail and every
/// sidebar assertion in it passes vacuously. Tests that mean to look at the
/// sidebar pass [`rail_visible_size`]; tests about the shell itself pass
/// [`HARNESS_DEFAULT`].
///
/// Deliberately unstyled: this suite's whole subject is band geometry read off
/// painted rects, and the embedded font stack would move those rects. `max_steps` is
/// 1024 against kittest's default of 4, which the styled constructor cannot express.
fn harness_at(state: AppState, width: f32, height: f32) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(width, height), 1024)
}

#[track_caller]
fn manual_refresh(h: &mut Harness<'_, AppState>) {
    h.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(h);
}

#[test]
fn tab_strip_is_the_content_columns_topmost_band() {
    let parent = test_support::scratch_root("workspace-shell-frame-topband");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-topband");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    let label = galley_origin(&h, "Changes").expect("the active tab label paints");

    let content_left = tab_underlines(&h)
        .into_iter()
        .map(|rect| rect.left())
        .fold(f32::INFINITY, f32::min);
    assert!(
        content_left.is_finite(),
        "the active tab's accent underline locates the content column's leading \
         edge; nothing brand-filled painted in the strip"
    );
    let mut above: Vec<String> = painted_galleys(&h)
        .iter()
        .filter(|g| g.rect.left() >= content_left && g.rect.top() < label.y - 0.5)
        .map(|g| format!("text {:?} at {:?}", g.text, g.rect))
        .collect();
    above.extend(
        filled_rects(&h)
            .iter()
            .filter(|(r, _)| r.left() >= content_left && r.top() < label.y - 0.5)
            .map(|(r, _)| format!("rect {r:?}")),
    );
    assert!(
        above.is_empty(),
        "the tab strip must be the content column's topmost band; painted above it: {above:#?}"
    );
}

#[test]
fn center_tabs_include_changes_log_branches_worktrees_submodules() {
    let parent = test_support::scratch_root("workspace-shell-frame-tabs");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-tabs");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&h, label);
    }
}

#[test]
fn unimplemented_tabs_render_empty_state_placeholder() {
    let parent = test_support::scratch_root("workspace-shell-frame-placeholder");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-placeholder");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.tab = turbogit_app::state::Tab::Branches;
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "Branches");
}

fn tab_strip_band(harness: &Harness<'_, AppState>) -> Rect {
    let label: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == "Changes")
        .collect();
    assert_eq!(label.len(), 1, "one `Changes` label; found {label:#?}");
    let centre = label[0].rect.center();
    let viewport = harness.ctx.content_rect();
    Rect::from_center_size(
        egui::pos2(viewport.center().x, centre.y),
        egui::vec2(viewport.width(), TAB_STRIP_HEIGHT),
    )
}

fn tab_underlines(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    let strip = tab_strip_band(harness);
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, color)| {
            *color == Palette::BRAND
                && (rect.height() - RAIL_WIDTH).abs() < 0.01
                && strip.expand(1.0).intersect(*rect) == *rect
        })
        .map(|(rect, _)| rect)
        .collect()
}

#[test]
fn the_active_tab_is_an_accent_underline_and_a_brighter_label() {
    let parent = test_support::scratch_root("workspace-shell-frame-pill");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-pill");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let state = app_state(&project_dir, &[repo]);
    let mut h = harness(state);
    settle(&mut h);

    let underlines = tab_underlines(&h);
    assert_eq!(
        underlines.len(),
        1,
        "the active tab paints one accent underline in the strip: {underlines:?}"
    );
    let rule = underlines[0];
    let strip = tab_strip_band(&h);
    assert_eq!(
        rule.height(),
        RAIL_WIDTH,
        "the underline is the token layer's accent width, not a literal at the call site"
    );
    assert!(
        (rule.bottom() - strip.bottom()).abs() <= 1.0,
        "the underline runs along the strip's bottom edge: rule {rule:?}, strip {strip:?}"
    );

    let strip_ink = |harness: &Harness<'_, AppState>, label: &str| -> Color32 {
        let hits: Vec<_> = painted_galleys(harness)
            .into_iter()
            .filter(|g| g.text == label && strip.contains_rect(g.rect))
            .collect();
        assert_eq!(hits.len(), 1, "one `{label}` in the strip, found {hits:#?}");
        hits.into_iter().next().expect("one hit").color
    };
    let active = strip_ink(&h, "Changes");
    assert_eq!(
        active,
        Palette::INK,
        "the active tab's label is the brightest ink on the ramp"
    );
    for quiet in ["Log", "Branches", "Worktrees", "Submodules"] {
        assert_eq!(
            strip_ink(&h, quiet),
            Palette::INK_3,
            "an inactive tab is quiet on the shared ramp, one step down from the active one"
        );
    }

    let label_rect = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "Changes")
        .expect("the active tab label")
        .rect;
    let strip = tab_strip_band(&h);
    let marks: Vec<(Rect, Color32)> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| {
            rect.intersect(strip.expand(1.0)) == *rect && rect.intersect(label_rect).is_positive()
        })
        .collect();
    for (rect, color) in &marks {
        assert_eq!(
            *color,
            Palette::BRAND,
            "the active tab is marked by its accent underline and a brighter \
             label, and by nothing else; the strip painted {color:?} across the \
             label in {rect:?}"
        );
        assert_eq!(
            rect.height(),
            RAIL_WIDTH,
            "the only thing the strip paints across a tab is the underline: a \
             full-height mark would be a second treatment. Painted {rect:?}"
        );
    }
    assert!(
        !filled_rects(&h).iter().any(|(r, c)| {
            *c == Palette::SURFACE && r.contains(label_rect.center()) && r.height() > 28.0
        }),
        "active tab must not paint a full-width box outline behind its label"
    );
}

#[test]
fn exactly_one_tab_carries_the_accent_underline() {
    let parent = test_support::scratch_root("workspace-shell-frame-one-underline");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-one-underline");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");

    for tab in [
        turbogit_app::state::Tab::Commit,
        turbogit_app::state::Tab::Log,
        turbogit_app::state::Tab::Branches,
        turbogit_app::state::Tab::Worktrees,
        turbogit_app::state::Tab::Submodules,
    ] {
        let mut state = app_state(&project_dir, std::slice::from_ref(&repo));
        state.ui.tab = tab;
        let mut h = harness(state);
        settle(&mut h);
        let underlines = tab_underlines(&h);
        assert_eq!(
            underlines.len(),
            1,
            "with {:?} active exactly one tab may carry the accent underline, found \
             {underlines:?}",
            tab
        );
    }
}

#[test]
fn shell_is_two_zones_without_metadata_rail() {
    let parent = test_support::scratch_root("workspace-shell-frame-metadata");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-metadata");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);

    assert_not_painted(&h, "METADATA");
    assert_not_painted(&h, "Upstream");
    for old in [
        "File\0",
        "Edit\0",
        "View\0",
        "Navigate\0",
        "Code\0",
        "Window\0",
        "Help\0",
        "Run\0",
        "Debug\0",
        "Update Project\0",
    ] {
        assert_not_painted(&h, old);
    }
    for old in ["Fetch", "Pull", "Push", "More"] {
        assert_not_painted(&h, old);
    }

    assert_painted(&h, "1 total");
}

#[test]
fn status_bar_shows_aggregated_total_repos() {
    let parent = test_support::scratch_root("workspace-shell-frame-statusbar");
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "1 total");
}

#[test]
fn status_bar_shows_diverged_count_when_root_is_ahead_of_upstream() {
    let parent = test_support::scratch_root("workspace-shell-frame-statusbar-diverged");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-statusbar-diverged");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ahead 1"]);

    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);
    assert_painted(&h, "1 diverged");
}

#[test]
fn status_bar_paints_unpulled_dirty_and_scope_from_real_data() {
    let parent = test_support::scratch_root("workspace-shell-frame-statusbar-unpulled");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-statusbar-unpulled");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let dirty = temp_repo(&parent, "dirty");
    let behind = temp_repo(&parent, "behind");
    let bare = parent.join("behind.origin");
    let seed = parent.join("behind-seed");
    let _ = std::fs::remove_dir_all(&seed);
    git(
        &parent,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    );
    std::fs::write(seed.join("remote.txt"), "remote\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "remote only"]);
    git(&seed, &["push", "-q", "origin", "main"]);
    git(&behind, &["fetch", "-q"]);
    std::fs::write(dirty.join("wip.txt"), "wip\n").unwrap();

    let mut state = app_state(&project_dir, &[dirty, behind]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    assert_painted(&h, "1 unpulled");
    assert_painted(&h, "1 dirty");
    assert_painted(&h, "1 repo in scope");
    assert_painted(&h, "2 total");
}

fn busy_status_bar(tag: &str) -> (Harness<'static, AppState>, PathBuf) {
    let parent = test_support::scratch_root(&format!("workspace-shell-frame-ramp-{tag}"));
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join(format!("wsf-ramp-{tag}"));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let dirty = temp_repo(&parent, "dirty");
    let behind = temp_repo(&parent, "behind");
    let bare = parent.join("behind.origin");
    let seed = parent.join("behind-seed");
    let _ = std::fs::remove_dir_all(&seed);
    git(
        &parent,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    );
    std::fs::write(seed.join("remote.txt"), "remote\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "remote only"]);
    git(&seed, &["push", "-q", "origin", "main"]);
    git(&behind, &["fetch", "-q"]);
    std::fs::write(dirty.join("wip.txt"), "wip\n").unwrap();

    let mut state = app_state(&project_dir, &[dirty, behind]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);
    (h, parent)
}

fn is_an_aggregated_counter(text: &str) -> bool {
    text.ends_with(" diverged")
        || text.ends_with(" conflict")
        || text.ends_with(" unpulled")
        || text.ends_with(" archived")
        || text.ends_with(" dirty")
}

fn status_bar_band(harness: &Harness<'_, AppState>) -> Rect {
    let origin = galley_origin(harness, "2 total").expect("the status bar paints");
    let bottom = harness.ctx.content_rect().bottom();
    Rect::from_min_max(egui::pos2(0.0, origin.y - 8.0), egui::pos2(bottom, bottom))
}

fn status_bar_inks(harness: &Harness<'_, AppState>) -> Vec<(String, Color32)> {
    let band = status_bar_band(harness);
    painted_galleys(harness)
        .into_iter()
        .filter(|g| band.contains_rect(g.rect))
        .map(|g| (g.text.clone(), g.color))
        .collect()
}

#[test]
fn the_status_bar_sits_one_step_up_the_ink_ramp_on_its_raised_band() {
    let (h, _parent) = busy_status_bar("ink");
    let inks = status_bar_inks(&h);
    assert!(
        inks.len() >= 6,
        "the fixture must exercise the whole bar, not one line: {inks:#?}"
    );
    for (text, ink) in &inks {
        if is_an_aggregated_counter(text) {
            continue;
        }
        assert_eq!(
            *ink,
            Palette::INK_2,
            "`{text}` is quiet text on the status bar's raised `SURFACE` band, \
             where the muted step is illegal (4.20:1) and the ink steps up to \
             INK_2 (6.58:1); it painted {ink:?}"
        );
    }
    for (text, ink) in &inks {
        assert_ne!(
            *ink,
            Palette::INK_3,
            "the muted step is not legal on the status bar's raised band, and \
             `{text}` painted it"
        );
        assert_ne!(
            *ink,
            Palette::INK_4,
            "the dim step is for placeholders and hatches only; `{text}` painted \
             it on the status bar"
        );
    }
}

#[test]
fn the_status_bar_counters_read_the_one_repository_state_colour_map() {
    let (h, _parent) = busy_status_bar("state");
    let inks = status_bar_inks(&h);
    let ink_of = |needle: &str| -> Color32 {
        let hits: Vec<_> = inks.iter().filter(|(text, _)| text == needle).collect();
        assert_eq!(hits.len(), 1, "one `{needle}` on the bar, found {inks:#?}");
        hits[0].1
    };
    assert_eq!(
        ink_of("1 unpulled"),
        RepoState::Unpulled.color(),
        "the unpulled counter wears the one state map's unpulled colour"
    );
    assert_eq!(
        ink_of("1 dirty"),
        RepoState::Dirty.color(),
        "the dirty counter wears the one state map's dirty colour"
    );
    assert_eq!(
        RepoState::Unpulled.color(),
        Palette::COUNTER,
        "and the map's unpulled colour IS the reserved counter orange"
    );
    for (text, ink) in &inks {
        if *ink == Palette::COUNTER {
            assert!(
                text.ends_with("unpulled") || text.ends_with("dirty"),
                "the reserved counter orange is for dirt and unpushed counts \
                 only, and `{text}` painted it"
            );
        }
    }
}

#[test]
fn the_shell_renders_at_the_harness_default_size_without_clipping() {
    let parent = test_support::scratch_root("workspace-shell-frame-noclip");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-noclip");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let mut state = app_state(&project_dir, &[repo]);
    state.ui.show_status_bar = true;
    let mut h = harness(state);
    settle(&mut h);

    let viewport = h.ctx.content_rect();
    assert_eq!(
        (viewport.width(), viewport.height()),
        HARNESS_DEFAULT,
        "this test is about the harness's own default size"
    );
    // Nothing the shell paints escapes the viewport's content area, and nothing
    // lands outside it. The frame carries an 8-point margin, so the inner rect
    // is the one content has to stay inside.
    let inner = viewport.shrink(8.0);
    for (rect, color) in filled_rects(&h) {
        assert!(
            inner.intersect(rect) == rect,
            "a filled rect escapes the shell's content area: {rect:?} in {color:?}, \
             inner {inner:?}"
        );
    }
    for galley in painted_galleys(&h) {
        assert!(
            inner.intersect(galley.rect) == galley.rect,
            "text clips or escapes at the default size: {:?} at {:?}, inner {inner:?}",
            galley.text,
            galley.rect
        );
    }
    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&h, label);
    }
    assert_painted(&h, "1 total");
    assert_eq!(
        tab_underlines(&h).len(),
        1,
        "the default size marks exactly one active tab"
    );
}

#[test]
fn the_harness_renders_the_sidebar_at_a_width_that_shows_it() {
    let parent = test_support::scratch_root("workspace-shell-frame-railing");
    let _ = std::fs::remove_dir_all(&parent);
    let parent = parent.parent().unwrap().join("wsf-railing");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    let project_dir = parent.clone();
    let repo = temp_repo(&parent, "alpha");
    let (w, h_) = rail_visible_size();
    let mut state = app_state(&project_dir, std::slice::from_ref(&repo));
    state.ui.show_status_bar = true;
    let mut h = harness_at(state, w, h_);
    settle(&mut h);

    let rail = filled_rects(&h)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::SIDEBAR
                && (rect.width() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 0.5
        })
        .map(|(rect, _)| rect)
        .expect("the sidebar paints its own surface at the rail's width");
    let viewport = h.ctx.content_rect();
    assert!(
        rail.left() - viewport.left() < 32.0,
        "the rail is flush with the content area's leading edge: {rail:?} vs {viewport:?}"
    );
    assert!(
        rail.right() < viewport.center().x,
        "the rail occupies the left of the content area, not the middle: {rail:?}"
    );
    assert_painted(&h, "PROJECTS");
    assert_painted(&h, "Filter repos / branches…");

    let mut narrow = harness_at(
        {
            let mut s = app_state(&project_dir, std::slice::from_ref(&repo));
            s.ui.show_status_bar = true;
            s
        },
        HARNESS_DEFAULT.0,
        HARNESS_DEFAULT.1,
    );
    settle(&mut narrow);
    assert!(
        filled_rects(&narrow)
            .iter()
            .any(|(_, c)| *c == Palette::SIDEBAR),
        "the default width clears the rail's threshold — if this ever changes, \
         `SIDEBAR_VISIBLE_FROM` and the no-clip test above need re-measuring"
    );
    let mut just_under = harness_at(
        {
            let mut s = app_state(&project_dir, std::slice::from_ref(&repo));
            s.ui.show_status_bar = true;
            s
        },
        SIDEBAR_VISIBLE_FROM - 20.0,
        HARNESS_DEFAULT.1,
    );
    settle(&mut just_under);
    assert!(
        !filled_rects(&just_under)
            .iter()
            .any(|(_, c)| *c == Palette::SIDEBAR),
        "20 points below the measured threshold the rail is still hidden"
    );
    let mut just_over = harness_at(
        {
            let mut s = app_state(&project_dir, &[repo]);
            s.ui.show_status_bar = true;
            s
        },
        SIDEBAR_VISIBLE_FROM,
        HARNESS_DEFAULT.1,
    );
    settle(&mut just_over);
    assert!(
        filled_rects(&just_over)
            .iter()
            .any(|(_, c)| *c == Palette::SIDEBAR),
        "{SIDEBAR_VISIBLE_FROM} is the narrowest width that shows the rail"
    );
}

#[test]
fn the_narrow_window_collapse_is_a_second_render_that_keeps_the_navigation() {
    let (mut h, _parent) = busy_status_bar("narrow");
    h.set_size(egui::vec2(rail_visible_size().0, rail_visible_size().1));
    settle(&mut h);
    assert_painted(&h, "PROJECTS");
    let wide_viewport = h.ctx.content_rect();
    assert!(wide_viewport.width() >= shell::MIN_SIDEBAR_WINDOW_WIDTH);

    h.set_size(egui::vec2(SIDEBAR_VISIBLE_FROM - 300.0, 560.0));
    settle(&mut h);
    let narrow = h.ctx.content_rect();
    assert!(
        narrow.width() < shell::MIN_SIDEBAR_WINDOW_WIDTH,
        "the second render must actually be narrow, or this test proves nothing: \
         {} wide",
        narrow.width()
    );
    assert_not_painted(&h, "PROJECTS");
    assert_not_painted(&h, "Filter repos / branches…");
    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&h, label);
    }
    assert_eq!(
        tab_underlines(&h).len(),
        1,
        "the collapsed render marks exactly one active tab too"
    );
    {
        h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A);
        settle(&mut h);
        assert!(
            h.state().ui.command_palette,
            "the palette route to switching workspaces is still reachable at a \
             narrow width"
        );
        assert_painted(&h, "Switch Workspace…");
        h.key_press(egui::Key::Escape);
        settle(&mut h);
    }
    let inner = narrow.shrink(8.0);
    for (rect, _color) in filled_rects(&h) {
        assert!(
            inner.intersect(rect) == rect,
            "the collapsed render clips: {rect:?} escapes {inner:?}"
        );
    }
    for galley in painted_galleys(&h) {
        assert!(
            inner.intersect(galley.rect) == galley.rect,
            "the collapsed render clips text: {:?} at {:?} escapes {inner:?}",
            galley.text,
            galley.rect
        );
    }
    assert!(
        painted_ink(&h, "2 total").is_some_and(|ink| ink == Palette::INK_2),
        "the status bar's total keeps the step-up ink in the collapsed render too"
    );
}
