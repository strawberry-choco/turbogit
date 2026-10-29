#![cfg(feature = "harness")]

use egui::Vec2;
use egui_kittest::Harness;
use test_support::harness::{
    assert_painted, filled_rects, painted_galleys, painted_text, settle, settle_quiet,
    shell_harness, shell_harness_over, shell_harness_over_animated, shell_harness_over_unstyled,
    widget_harness,
};
use turbogit_app::state::{AppState, Toast};

fn own_state() -> (AppState, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("temp project dir");
    let cfg = tempfile::tempdir().expect("temp config dir");
    let cfg_path = cfg.path().to_path_buf();
    std::mem::forget(cfg);
    let state = AppState::launch_in(Some(project.path().to_path_buf()), Some(cfg_path));
    (state, project)
}

fn widest_fill(harness: &Harness<'_, AppState>) -> f32 {
    filled_rects(harness)
        .iter()
        .map(|(rect, _)| rect.width())
        .fold(0.0f32, f32::max)
}

fn box_of<S>(harness: &Harness<'_, S>) -> Vec2 {
    harness
        .input()
        .screen_rect
        .expect("the constructor set a screen rect")
        .size()
}

#[test]
fn the_size_taking_constructor_lays_the_shell_out_in_the_requested_box() {
    let (state, _project) = own_state();
    let size = Vec2::new(1280.0, 800.0);
    let mut wide = shell_harness_over(state, size);
    assert_eq!(
        box_of(&wide),
        size,
        "the box the caller asked for is the box the harness was given"
    );
    wide.step();

    let (narrow_state, _p) = own_state();
    let mut narrow = shell_harness_over(narrow_state, Vec2::new(700.0, 800.0));
    narrow.step();
    assert!(
        widest_fill(&wide) > widest_fill(&narrow),
        "a 1280-wide box must lay the shell out wider than a 700-wide one: {} vs {}",
        widest_fill(&wide),
        widest_fill(&narrow)
    );
    assert!(
        !painted_galleys(&wide).is_empty(),
        "the frame painted no text at all"
    );
}

#[test]
fn the_size_taking_constructor_and_the_default_one_agree_on_the_frame() {
    let (sized, _p) = own_state();
    let mut sized = shell_harness_over(sized, Vec2::new(1024.0, 768.0));
    let (mut dflt, _project) = shell_harness();
    sized.step();
    dflt.step();
    assert_eq!(
        painted_text(&sized),
        painted_text(&dflt),
        "the same state at the same box must paint the same text through either \
         constructor"
    );
    assert_eq!(
        widest_fill(&dflt),
        widest_fill(&sized),
        "and the same geometry"
    );
}

const MARKER: &str = "iiiiiiiiiiiiiiii";

fn unfonited(size: Vec2) -> Harness<'static, ()> {
    let mut harness = Harness::new_ui_state(
        |ui, _| {
            turbogit_ui::theme::configure_style(ui.ctx());
            egui::CentralPanel::default().show(ui, |ui| {
                ui.monospace(MARKER);
            });
        },
        (),
    );
    harness.set_size(size);
    harness.step();
    harness
}

fn monospace_width(harness: &Harness<'static, ()>) -> f32 {
    painted_galleys(harness)
        .iter()
        .find(|g| g.text == MARKER)
        .unwrap_or_else(|| panic!("{MARKER:?} painted; got {:?}", painted_galleys(harness)))
        .rect
        .width()
}

#[test]
fn the_widget_constructor_installs_the_embedded_fonts_itself() {
    let mut harness = widget_harness(Vec2::new(400.0, 200.0), |ui| {
        ui.monospace(MARKER);
    });
    harness.step();

    let installed = monospace_width(&harness);
    let bare = monospace_width(&unfonited(Vec2::new(400.0, 200.0)));
    assert_ne!(
        installed, bare,
        "the shared constructor must install the embedded JetBrains Mono itself: \
         its monospace advance is {installed}, an un-installed frame's is {bare}"
    );
}

#[test]
fn the_widget_constructor_paints_the_body_it_is_handed() {
    let mut harness = widget_harness(Vec2::new(360.0, 120.0), |ui| {
        ui.monospace("marker-one");
        ui.monospace("marker-two");
    });
    assert_eq!(
        box_of(&harness),
        Vec2::new(360.0, 120.0),
        "at the caller's box"
    );
    harness.step();
    assert_painted(&harness, "marker-one");
    assert_painted(&harness, "marker-two");
}

fn scripted(
    script: &[&'static str],
) -> (
    egui_kittest::Harness<'static, ()>,
    std::rc::Rc<std::cell::Cell<usize>>,
) {
    let frame = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let f = std::rc::Rc::clone(&frame);
    let script = script.to_vec();
    let harness = widget_harness(Vec2::new(300.0, 120.0), move |ui| {
        let n = f.get();
        f.set(n + 1);
        ui.monospace(script[n.min(script.len() - 1)]);
    });
    (harness, frame)
}

#[test]
fn the_long_settle_waits_past_a_change_where_the_short_one_stops_at_the_first_repeat() {
    let (mut short, frame) = scripted(&["A", "A", "B"]);
    frame.set(0);
    settle(&mut short);
    assert_eq!(
        painted_text(&short),
        vec!["A".to_string()],
        "the 10-frame settle stops at the FIRST repeat — that is what its callers \
         wait for, and it is why the long one may not be merged into it"
    );

    let (mut long, frame) = scripted(&["A", "A", "B"]);
    frame.set(0);
    settle_quiet(&mut long);
    assert_eq!(
        painted_text(&long),
        vec!["B".to_string()],
        "the long settle must not return on a frame that is about to change"
    );
    assert!(
        frame.get() > 5,
        "and it got there by stepping, not by waiting: {} frames",
        frame.get()
    );
}

#[test]
#[should_panic(expected = "did not settle within 300 frames")]
fn the_long_settle_gives_up_at_its_budget_rather_than_forever() {
    let script: Vec<&'static str> = (0..400)
        .map(|i| if i % 2 == 0 { "a" } else { "b" })
        .collect();
    let (mut harness, _frame) = scripted(&script);
    settle_quiet(&mut harness);
}

const BOX: Vec2 = Vec2::new(1024.0, 768.0);

fn zero_root_state() -> AppState {
    let (state, project) = own_state();
    std::mem::forget(project);
    state
}

fn busy_state() -> AppState {
    let mut state = zero_root_state();
    state.ui.busy = true;
    state
}

const TOAST_TEXT: &str = "mid-fade probe";

fn open_toast(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.toast = Some(Toast::success(TOAST_TEXT));
}

fn mid_fade_alphas<S>(harness: &Harness<'_, S>, toast_text: &str) -> Vec<u8> {
    let Some(text) = painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == toast_text)
    else {
        return Vec::new();
    };
    filled_rects(harness)
        .iter()
        .filter(|(rect, _)| rect.min.y < text.rect.max.y && rect.max.y > text.rect.min.y)
        .map(|(_, fill)| fill.a())
        .filter(|a| *a > 0 && *a < u8::MAX)
        .collect()
}

#[test]
#[should_panic(expected = "exceeded max_steps (4)")]
fn the_shared_constructor_still_carries_kittests_four_step_budget() {
    let mut harness = shell_harness_over(busy_state(), BOX);
    harness.run();
}

#[test]
fn a_raised_budget_is_the_budget_the_run_really_gets() {
    let mut sized = shell_harness_over_unstyled(busy_state(), BOX, 32);
    let err = sized
        .try_run()
        .expect_err("a busy frame never settles, so the run ends on the budget");
    assert_eq!(
        err.max_steps, 32,
        "the budget the caller stated is the budget the run is held to"
    );

    let (default_state, _p) = own_state();
    let mut at_default = shell_harness_over(default_state, BOX);
    at_default.state_mut().ui.busy = true;
    at_default.try_run().expect_err("busy");
    assert!(
        at_default.ctx.input(|i| i.time) < 2.0,
        "four quarter-second steps plus the constructor's own frames: {}",
        at_default.ctx.input(|i| i.time)
    );
    assert!(
        sized.ctx.input(|i| i.time) >= 8.0,
        "thirty-two quarter-second steps: {}",
        sized.ctx.input(|i| i.time)
    );

    let mut looping = shell_harness_over_unstyled(busy_state(), BOX, 32);
    let mut elapsed = 0.0f64;
    for call in 1..=7 {
        looping.try_run().expect_err("busy");
        let now = looping.ctx.input(|i| i.time);
        assert!(
            now - elapsed >= 8.0,
            "call {call} advanced the clock by {}s, not a full 32-step budget",
            now - elapsed
        );
        elapsed = now;
    }
}

#[test]
fn the_animated_constructor_steps_the_clock_the_default_would_skip_past() {
    let (slow_state, _p) = own_state();
    let slow = shell_harness_over(slow_state, BOX);
    let fast = shell_harness_over_animated(zero_root_state(), BOX);
    assert_eq!(
        slow.input().predicted_dt,
        0.25,
        "kittest's default is a quarter of a second per step"
    );
    assert_eq!(
        fast.input().predicted_dt,
        1.0 / 60.0,
        "the animated constructor steps at sixty frames a second"
    );
}

#[test]
fn a_fade_at_sixty_fps_is_still_in_flight_where_the_default_clock_finishes_it() {
    let mut fast = shell_harness_over_animated(zero_root_state(), BOX);
    open_toast(&mut fast);
    fast.step();
    fast.step();
    let slow_fades = {
        let mut slow = shell_harness_over(zero_root_state(), BOX);
        open_toast(&mut slow);
        slow.step();
        slow.step();
        mid_fade_alphas(&slow, TOAST_TEXT)
    };

    let fast_fades = mid_fade_alphas(&fast, TOAST_TEXT);
    assert!(
        !fast_fades.is_empty(),
        "at 1/60 s the window's fade must still be running: the frame painted no \
         translucent fill at all"
    );
    assert!(
        slow_fades.is_empty(),
        "at 1/4 s the same window is already fully painted — alphas {slow_fades:?} — \
         which is the whole reason the default clock cannot assert a fade"
    );
    assert!(
        fast_fades.iter().all(|a| *a > 0 && *a < u8::MAX),
        "and it is mid-fade, not painted or absent: {fast_fades:?}"
    );
}

#[test]
fn the_unstyled_constructor_installs_neither_the_fonts_nor_the_tokens() {
    let (styled_state, _p) = own_state();
    let mut styled = shell_harness_over(styled_state, BOX);
    let mut bare = shell_harness_over_unstyled(zero_root_state(), BOX, 32);
    styled.step();
    bare.step();

    let bold = egui::FontFamily::Name("jetbrains-mono-bold".into());
    assert!(
        styled.ctx.fonts(|f| f.families().contains(&bold)),
        "the styling constructor installs the embedded stack"
    );
    assert!(
        !bare.ctx.fonts(|f| f.families().contains(&bold)),
        "the no-style constructor must install no fonts: its families are {:?}",
        bare.ctx.fonts(|f| f.families())
    );

    let stroke = |ctx: &egui::Context| ctx.style_of(egui::Theme::Dark).visuals.window_stroke;
    assert_eq!(
        stroke(&styled.ctx).color,
        turbogit_ui::theme::Palette::LINE,
        "the styling constructor applies the design system's line token"
    );
    assert_ne!(
        stroke(&bare.ctx).color,
        turbogit_ui::theme::Palette::LINE,
        "the no-style constructor must leave egui's own window stroke — the frame \
         a suite that asserts painted geometry deliberately measures — in place"
    );
}

#[test]
fn the_unstyled_constructor_measures_the_same_text_differently() {
    let (styled_state, _p) = own_state();
    let mut styled = shell_harness_over(styled_state, BOX);
    let mut bare = shell_harness_over_unstyled(zero_root_state(), BOX, 32);
    styled.step();
    bare.step();

    let widths = |harness: &Harness<'_, AppState>| -> std::collections::BTreeMap<String, f32> {
        painted_galleys(harness)
            .iter()
            .map(|g| (g.text.clone(), g.rect.width()))
            .collect()
    };
    let (styled_widths, bare_widths) = (widths(&styled), widths(&bare));
    let shared: Vec<(&String, f32, f32)> = styled_widths
        .iter()
        .filter_map(|(text, w)| bare_widths.get(text).map(|b| (text, *w, *b)))
        .collect();
    assert!(
        shared.len() >= 10,
        "the two frames must share enough strings for the comparison to mean \
         something, but they shared {}",
        shared.len()
    );
    let remeasured: Vec<&(&String, f32, f32)> = shared.iter().filter(|(_, a, b)| a != b).collect();
    assert!(
        !remeasured.is_empty(),
        "the same strings measured the same width under both constructors: {:?}",
        shared
    );
}

#[test]
fn every_shell_constructor_installs_the_fonts_and_only_one_place_does() {
    for (what, harness) in [
        (
            "shell_harness_over",
            shell_harness_over(zero_root_state(), BOX) as Harness<'_, AppState>,
        ),
        (
            "shell_harness_over_animated",
            shell_harness_over_animated(zero_root_state(), BOX),
        ),
    ] {
        let bold = egui::FontFamily::Name("jetbrains-mono-bold".into());
        assert!(
            harness.ctx.fonts(|f| f.families().contains(&bold)),
            "{what} must install the embedded stack even though an earlier harness \
             in this same process already did"
        );
    }

    let harness_src = test_support::srcscan::read_source(crate_src().join("harness.rs"));
    let mut owners = test_support::srcscan::fns_using(&harness_src, "install_fonts");
    owners.sort();
    assert_eq!(
        owners,
        vec![
            "shell_harness_built".to_owned(),
            "widget_harness".to_owned()
        ],
        "`install_fonts` must be reached from the two shared constructors and \
         nothing else: found {owners:?}"
    );
    assert_eq!(
        test_support::srcscan::call_sites(crate_src(), "install_fonts").len(),
        2,
        "once each, so the flag in each is the only thing keeping the install \
         once-only"
    );

    let mut routed = test_support::srcscan::fns_using(&harness_src, "shell_harness_built");
    routed.sort();
    assert_eq!(
        routed,
        vec![
            "shell_harness_over".to_owned(),
            "shell_harness_over_animated".to_owned(),
            "shell_harness_over_unstyled".to_owned(),
        ],
        "every public shell constructor must delegate to the shared preamble: \
         found {routed:?}"
    );
}

fn crate_src() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}
