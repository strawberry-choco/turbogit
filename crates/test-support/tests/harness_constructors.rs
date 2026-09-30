//! The shared harness constructors are the preamble every suite in the tree
//! used to hand-roll, so what they *guarantee* is load-bearing: a suite that
//! adopts one must not silently change the frame it measures. Each test here
//! pins a claim from the constructor's doc comment against what the frame
//! actually did — the requested box, the real shell render, and the embedded
//! fonts.
//!
//! Gated on the `harness` feature with the constructors themselves.
#![cfg(feature = "harness")]

use egui::Vec2;
use egui_kittest::Harness;
use test_support::harness::{
    assert_painted, filled_rects, painted_galleys, painted_text, settle, settle_quiet,
    shell_harness, shell_harness_over, shell_harness_over_animated, shell_harness_over_unstyled,
    widget_harness,
};
use turbogit_app::state::{AppState, Toast};

/// A zero-root state of the caller's own, plus a config dir that outlives it.
fn own_state() -> (AppState, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("temp project dir");
    let cfg = tempfile::tempdir().expect("temp config dir");
    let cfg_path = cfg.path().to_path_buf();
    std::mem::forget(cfg);
    let state = AppState::launch_in(Some(project.path().to_path_buf()), Some(cfg_path));
    (state, project)
}

/// The widest filled rect of one frame — a shell region's horizontal extent.
fn widest_fill(harness: &Harness<'_, AppState>) -> f32 {
    filled_rects(harness)
        .iter()
        .map(|(rect, _)| rect.width())
        .fold(0.0f32, f32::max)
}

/// The box a constructor handed egui, read before the first `step` — that call
/// consumes `screen_rect` out of the input and hands it to the context.
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

    // And the layout RESPONDS to it, which is the claim that matters: a
    // constructor that recorded the size but rendered at 1024 would pass the
    // line above.
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
    // The default constructor is the size-taking one at the default box: a
    // second implementation would be a second set of preamble rules.
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

/// The string whose monospace advance the embedded font changes.
const MARKER: &str = "iiiiiiiiiiiiiiii";

/// A bare widget harness, built by hand with NO `install_fonts`, for the
/// constructor to be measured against.
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

/// The advance width the frame gave one monospace string — the measurement the
/// embedded font changes and the default one does not.
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
    // The premise of putting the once-only install in the shared constructor:
    // `install_fonts` per frame would change what the shell paints with nothing
    // failing. So prove the install HAPPENED, by measuring the monospace
    // metrics it produces against a frame that never installed anything.
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

/// A bare harness whose painted text follows `script` frame by frame, holding
/// the last entry forever once the script runs out, plus the frame counter the
/// caller can rewind.
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
    // "A" twice then "B" forever. A ONE-repeat wait is satisfied by the second A
    // and returns still showing A — a frame that changes on the very next step.
    // A THREE-consecutive wait has to sit through the change. That is the whole
    // difference between the two budgets, so it is the thing pinned here.
    //
    // The counter is rewound first: `Harness::new_ui_state` runs three frames of
    // its own before handing the harness over, and those are not the test's
    // frames — left alone they would consume the script before either wait
    // began.
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
    // Never the same twice in a row, for longer than the budget: with no budget
    // this is a hung suite, not a failure. The script is longer than 300 so the
    // harness's hold-the-last-entry rule cannot turn it constant.
    let script: Vec<&'static str> = (0..400)
        .map(|i| if i % 2 == 0 { "a" } else { "b" })
        .collect();
    let (mut harness, _frame) = scripted(&script);
    settle_quiet(&mut harness);
}

// --- The seams ticket 06's three lanes hit independently --------------------
//
// Three suites in the tree cannot be written as a call to `shell_harness_over`,
// and each found that out alone:
// - **`max_steps`** — kittest's budget is 4 and `Harness::run` *panics* past it,
//   so a suite that calls `run()` in a loop has to state its own.
// - **`step_dt`** — kittest's is 1/4 s and becomes the frame's `predicted_dt`,
//   which is what egui integrates every fade against, so a test asserting a fade
//   is *mid-flight* needs a clock that leaves it there.
// - **no styling** — a suite asserting painted strings or rect geometry must not
//   get the design system's tokens and embedded fonts; they re-measure exactly
//   what it asserts.
//
// Each is its own constructor rather than a knob, for the reason
// `settle`/`settle_quiet` above give.

/// The box these seam tests lay the shell out in. Stated rather than inherited,
/// like every other box in this file.
const BOX: Vec2 = Vec2::new(1024.0, 768.0);

/// A zero-root state that outlives this function, so a helper can hand one to a
/// harness that outlasts the helper. The project dir is deliberately leaked for
/// the reason `shell_harness`'s is: a deleted dir makes a later read of it racy.
fn zero_root_state() -> AppState {
    let (state, project) = own_state();
    std::mem::forget(project);
    state
}

/// The same state with `busy` set — the shell's own "keep frames coming" flag
/// (`ui/shell.rs:192` turns it into `ctx.request_repaint()`), so a frame over it
/// **never settles** and a run on it ends on its budget.
///
/// A production path, not a synthetic one: a suite with an operation or a keyed
/// read in flight is in exactly this state, which is why 22 suites that call
/// `run()` hand-roll their own launcher.
fn busy_state() -> AppState {
    let mut state = zero_root_state();
    state.ui.busy = true;
    state
}

/// Open a toast **after** the harness exists, so the next frame is the frame the
/// window opened on. Planted in the state instead, the toast is already open
/// through the two frames `Harness::from_builder` runs of its own and the fade
/// is long finished by the time the test steps.
///
/// The window is the shell's own toast surface, and egui gives every `Area` a
/// fade-in over `style.animation_time`, which `theme::configure_style` sets to
/// 0.12 s — so its contents are painted at a partial alpha for as many frames
/// as the clock leaves them, which is the primitive
/// `shell_harness_over_animated` exists to keep in flight.
fn open_toast(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.toast = Some(Toast::success("mid-fade probe"));
}

/// The alpha of every filled rect the last frame painted that is neither fully
/// opaque nor fully transparent — i.e. what a frame caught mid-fade.
fn mid_fade_alphas<S>(harness: &Harness<'_, S>) -> Vec<u8> {
    filled_rects(harness)
        .iter()
        .map(|(_, fill)| fill.a())
        .filter(|a| *a > 0 && *a < u8::MAX)
        .collect()
}

#[test]
#[should_panic(expected = "exceeded max_steps (4)")]
fn the_shared_constructor_still_carries_kittests_four_step_budget() {
    // The default is a guarantee of its own, and a raised-budget *sibling* must
    // not quietly move it. Pin the number, not just the panic, so a sibling that
    // reached the shared builder and raised the default cannot pass this.
    let mut harness = shell_harness_over(busy_state(), BOX);
    harness.run();
}

#[test]
fn a_raised_budget_is_the_budget_the_run_really_gets() {
    // Read straight off the error kittest hands back, so this pins the number
    // the constructor handed IT rather than the absence of a panic.
    let mut sized = shell_harness_over_unstyled(busy_state(), BOX, 32);
    let err = sized
        .try_run()
        .expect_err("a busy frame never settles, so the run ends on the budget");
    assert_eq!(
        err.max_steps, 32,
        "the budget the caller stated is the budget the run is held to"
    );

    // …and that the number was *spent*, not just recorded. Every step advances
    // the simulated clock by `step_dt`, so a run held to 32 steps leaves the
    // context eight times further along than one held to kittest's 4 — which is
    // the claim the 22 blocked suites depend on. A budget of 4 is not a smaller
    // budget for a busy frame, it is a suite that goes red.
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

    // And the shape those suites have: a run over and over on a frame that keeps
    // repainting, each getting the whole stated budget. `try_run` rather than
    // `run` because a frame that never settles exhausts *any* budget and `run`
    // panics at the end of it by design — which is why those suites' frames
    // settle between their polls.
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
    // `step_dt` is not a speed knob, it is the value egui integrates animations
    // against: `_step` writes it into `input.predicted_dt` every frame.
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
    // The claim behind `shell_harness_over_animated`, reproduced on a real shell
    // surface rather than argued from `predicted_dt`. `configure_style` sets
    // `style.animation_time = 0.12`; a window that opened this frame is painted
    // at `remap_clamp(age, 0..=0.12)`, and the age is what the clock advanced.
    let mut fast = shell_harness_over_animated(zero_root_state(), BOX);
    open_toast(&mut fast);
    fast.step();
    fast.step();
    let slow_fades = {
        let mut slow = shell_harness_over(zero_root_state(), BOX);
        open_toast(&mut slow);
        slow.step();
        slow.step();
        mid_fade_alphas(&slow)
    };

    let fast_fades = mid_fade_alphas(&fast);
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

    // FONTS. `install_fonts` registers a named bold family for the embedded
    // JetBrains Mono; nothing else in the tree does, so its presence is a direct
    // read of whether the install ran — and of which context it ran in.
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

    // TOKENS. `configure_style` is the only thing in the tree that writes
    // `visuals.window_stroke` (theme.rs:692), so it reads the same way.
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
    // The reason the previous test is not a style preference. Both frames paint
    // the same strings; the embedded stack re-measures them, so an assertion on
    // a painted string's width or on the rect a click lands in is a measurement
    // of WHICH constructor ran. Differential, not a named string: a label can
    // come and go, the divergence cannot.
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
    // The per-harness flag is the STRICTER of the two guards the tree has used.
    // A `static ONCE` is process-wide, so the second harness in a test process
    // would skip the install entirely and lay out with a different glyph — the
    // suite's own `the_widget_constructor_installs_the_embedded_fonts_itself`
    // above is only green because this one is per harness.
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

    // …and the mechanical half. `install_fonts` is called from exactly TWO
    // functions in this crate and the pair is named, not counted: the shared
    // shell preamble, and `widget_harness` — which cannot use the shell one,
    // because a `Harness<'static, ()>` has no `AppState` to drain and no render
    // to drive. A third call site is a per-frame install, or a variant that
    // quietly kept its own flag, and either changes the frame silently.
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

    // And the anti-drift half: every shell constructor routes through the one
    // preamble. A future variant that assembles its own frame is exactly the
    // failure this crate exists to prevent, and it would pass every test above.
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

/// This crate's own `src` — what the install-site ratchet reads.
fn crate_src() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}
