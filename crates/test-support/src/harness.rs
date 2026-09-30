//! Headless egui shell harness (DDD split issue 09).
//!
//! Drives [`turbogit_ui::render`] over [`turbogit_app::state::AppState`]
//! through `egui_kittest` so each UI ticket can exercise the full IDE shell
//! without copying setup code. The harness runs over synthetic raw input
//! (no GPU / window / display server) and asserts only on public surfaces:
//! - **Painted output** — the frame's shapes from `FullOutput`, i.e. exactly
//!   what a software painter would fill (text galleys carry their strings;
//!   filled rects carry their geometry + token color).
//! - **State transitions** — public `AppState` fields after the frames.
//!
//! Gated behind the `harness` feature so `turbogit-app`'s tests keep using
//! the recording executor without paying `egui_kittest`'s compile cost.

use egui::epaint::TextShape;
use egui::{Color32, FontFamily, Pos2, Rect, Shape, Ui, Vec2};
use egui_kittest::Harness;
use turbogit_app::state::AppState;
use turbogit_ui::theme::{configure_style, install_fonts};

/// The full shell over the caller's `state` in a `size` box, at kittest's own defaults.
pub fn shell_harness_over(state: AppState, size: Vec2) -> Harness<'static, AppState> {
    shell_harness_built(state, size, Tempo::KITTEST_DEFAULT, true)
}

/// [`shell_harness_over`] at a 1/60 s `step_dt` — the clock a fade stays **in
/// flight** at. `step_dt` is not a speed knob: `_step` writes it into the frame's
/// `predicted_dt`, which is what egui integrates animations against, so an
/// `egui::Area` opened this frame paints at `remap_clamp(age, 0..=animation_time)`
/// with `animation_time = 0.12` (`theme.rs:725`). At kittest's 1/4 s that window
/// is **already fully painted on its first visible frame** — alpha exactly 1.0 —
/// so a fill or stroke assertion requiring alpha *strictly* below 1.0 can only pass
/// here. See `branch_context_menu.rs::the_branch_context_menu_keeps_the_float_stroke`.
pub fn shell_harness_over_animated(state: AppState, size: Vec2) -> Harness<'static, AppState> {
    shell_harness_built(state, size, Tempo::SIXTY_FPS, true)
}

/// [`shell_harness_over`] with neither tokens nor embedded fonts, and a
/// `max_steps` budget stated by the caller.
///
/// Unstyled because painted-string and rect assertions must measure the frame as egui
/// draws it: the embedded stack re-measures every string and the tokens move the
/// rects a click lands in. The event drain is still wanted and runs.
///
/// `max_steps` is required rather than defaulted because kittest's default is **4**
/// and `Harness::run` — unlike `Harness::step` — **panics** once a run exceeds it. A
/// shell with an operation in flight never settles, so those suites need hundreds of
/// steps; a hidden default would restore the very 4 this exists to remove.
pub fn shell_harness_over_unstyled(
    state: AppState,
    size: Vec2,
    max_steps: u64,
) -> Harness<'static, AppState> {
    shell_harness_built(
        state,
        size,
        Tempo {
            max_steps,
            ..Tempo::KITTEST_DEFAULT
        },
        false,
    )
}

/// The box kittest's `HarnessBuilder` starts at — a launcher's inherited default.
pub const KITTEST_DEFAULT_BOX: Vec2 = Vec2::new(800.0, 600.0);

/// The `[max_steps, step_dt]` pair, which kittest accepts only at build time.
#[derive(Clone, Copy)]
struct Tempo {
    max_steps: u64,
    step_dt: f32,
}

impl Tempo {
    /// kittest's own `HarnessBuilder` defaults: four steps of a quarter second.
    const KITTEST_DEFAULT: Self = Self {
        max_steps: 4,
        step_dt: 1.0 / 4.0,
    };
    /// A 1/60 s clock, budget untouched — see [`shell_harness_over_animated`].
    const SIXTY_FPS: Self = Self {
        step_dt: 1.0 / 60.0,
        ..Self::KITTEST_DEFAULT
    };
}

/// The shell preamble, spelled out once so the rules cannot drift between the
/// public constructors.
///
/// The once-only install must stay once-only: if `install_fonts` ran every frame, no
/// assertion in the tree would fail — the shell would simply lay out with different
/// glyph metrics. The flag is per-harness, not a `static`, which is process-wide.
fn shell_harness_built(
    state: AppState,
    size: Vec2,
    tempo: Tempo,
    styled: bool,
) -> Harness<'static, AppState> {
    let mut fonts_installed = false;
    let mut harness = Harness::builder()
        .with_max_steps(tempo.max_steps)
        .with_step_dt(tempo.step_dt)
        .build_ui_state(
            move |ui, state| {
                if styled {
                    configure_style(ui.ctx());
                    if !fonts_installed {
                        install_fonts(ui.ctx());
                        fonts_installed = true;
                    }
                }
                state.drain_events();
                turbogit_ui::render(ui, state);
            },
            state,
        );
    // `set_size` after construction, not `with_size`: kittest runs two frames of its
    // own inside the builder that `with_size` would lay out at the caller's box.
    harness.set_size(size);
    harness
}

/// A bare `Harness<'static, ()>` painting `body` into a [`egui::CentralPanel`] at
/// `size`. No `AppState` or drain, but the same tokens and fonts as
/// [`shell_harness_over`], so a widget is measured against the shell's glyph.
pub fn widget_harness(size: Vec2, body: impl Fn(&mut Ui) + 'static) -> Harness<'static, ()> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| body(ui));
        },
        (),
    );
    harness.set_size(size);
    harness
}

/// A harness rendering the full shell over a fresh [`AppState`].
///
/// The project dir is an empty temp directory: zero roots are discovered, so
/// the render is deterministic, no background git workers are spawned, and —
/// per the Welcome-vs-shell model (spec §9.2) — the central body shows the
/// Welcome placeholder while every shell region still renders.
pub fn shell_harness() -> (Harness<'static, AppState>, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("temp project dir");
    // Inject an empty throwaway config dir so the developer's real global
    // recents file never leaks into headless tests (ADR-0005 test seam —
    // `AppState` docs: "Tests inject a temp dir so the real user
    // configuration is never touched"). Deliberately leaked: the harness
    // outlives this function, and a deleted config dir would make later
    // recents reads/writes racy.
    let cfg = tempfile::tempdir().expect("temp config dir");
    let cfg_path = cfg.path().to_path_buf();
    std::mem::forget(cfg);
    let state = AppState::launch_in(Some(project.path().to_path_buf()), Some(cfg_path));
    assert!(
        state.multi.roots.is_empty(),
        "test project must discover no roots"
    );
    (shell_harness_over(state, Vec2::new(1024.0, 768.0)), project)
}

/// All text painted by the last completed frame.
pub fn painted_text<S>(harness: &Harness<'_, S>) -> Vec<String> {
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

/// Assert `needle` appears in some painted text galley.
#[track_caller]
pub fn assert_painted<S>(harness: &Harness<'_, S>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Assert `needle` appears in no painted text galley.
#[track_caller]
pub fn assert_not_painted<S>(harness: &Harness<'_, S>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

/// Paint-time origin of the first text galley painting exactly `text`.
///
/// Exact matching keeps distinct labels unambiguous ("Log" vs "Git Log").
/// Used to relate a label to the region that visually contains it (e.g. the
/// active tab's surface rect).
pub fn galley_origin<S>(harness: &Harness<'_, S>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
}

/// One painted text galley: what it says, where it was painted, in what color,
/// and in which font family.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedGalley {
    /// The galley's full string.
    pub text: String,
    /// Paint-time origin (top-left of the text).
    pub pos: Pos2,
    /// The painted extent — origin plus the galley's laid-out size. Where a
    /// column ends is as much a fact of the frame as where it starts.
    pub rect: Rect,
    /// The ink this string was **painted** with.
    ///
    /// NOT simply `job.sections[0].format.color`, which is the obvious-looking
    /// one-liner and the wrong one. A galley keeps the colour it was *laid out*
    /// with, and this codebase lays text out in `Color32::WHITE`, then applies
    /// the real ink at paint time by overriding the text colour
    /// (`Painter::galley_with_override_text_color`). Reading the layout colour
    /// alone therefore reports `WHITE` for every shared button, kit button,
    /// sidebar row, menu item and segmented option — a test built on it passes
    /// or fails for the wrong reason. [`paint_ink`] is the one place that
    /// resolves paint-time override first and laid-out colour second.
    pub color: Color32,
    /// The resolved face — `Proportional` for chrome, `Monospace` for data
    /// (branch and remote names).
    pub family: FontFamily,
}

/// The ink one painted text shape actually drew with — the single
/// implementation of the layout-vs-paint colour rule.
///
/// A galley keeps the color it was *laid out* with, so a widget that overrides
/// the color at paint time — which is how every shared button, kit button,
/// sidebar row and menu item takes its ink — is only visible through the text
/// shape's `override_text_color`. Prefer the override, and fall back to the
/// laid-out color.
///
/// `Color32::TRANSPARENT` is reserved for the degenerate galley that carries no
/// sections at all; "there is no such painted text" is the caller's business
/// to represent (see [`painted_ink`]'s `Option`).
fn paint_ink(shape: &TextShape) -> Color32 {
    shape
        .override_text_color
        .or_else(|| shape.galley.job.sections.first().map(|s| s.format.color))
        .unwrap_or(Color32::TRANSPARENT)
}

/// Every text galley painted by the last frame.
///
/// The single primitive behind the label queries above. A string usually paints
/// in more than one place — the current branch names both the repo header's
/// branch pill and a list row; a repo name appears in the sidebar, in the
/// header breadcrumb and in a section header — so ask for the color or face of
/// *this* occurrence by matching on `pos`, rather than trusting the order
/// shapes happen to arrive in. Color and font both land in the galley's layout
/// job, so a token-colored, face-correct label (the active branch's soft blue
/// monospace, a diverged branch's red) is assertable from painted output alone —
/// no reach into widget internals. The color is resolved by [`paint_ink`], so
/// it is the paint-time ink rather than the layout-time placeholder.
pub fn painted_galleys<S>(harness: &Harness<'_, S>) -> Vec<PaintedGalley> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) => {
                let section = shape.galley.job.sections.first();
                Some(PaintedGalley {
                    text: shape.galley.text().to_owned(),
                    pos: shape.pos,
                    rect: Rect::from_min_size(shape.pos, shape.galley.size()),
                    color: paint_ink(shape),
                    family: section
                        .map(|s| s.format.font_id.family.clone())
                        .unwrap_or(FontFamily::Proportional),
                })
            }
            _ => None,
        })
        .collect()
}

/// The ink the last frame painted `needle` in, matched exactly.
///
/// The single-string sibling of [`painted_galleys`]: where a suite wants "the
/// colour this one string was drawn with" rather than a list to search, this
/// answers it, using the same layout-vs-paint rule (see [`paint_ink`]).
///
/// Exact matching keeps distinct labels unambiguous, the same discipline
/// [`galley_origin`] uses to relate a label to the region containing it.
/// `None` means the frame painted no such text at all — a string that exists
/// nowhere reads the same as one that exists with a transparent ink, and
/// collapsing the two into a sentinel colour is how a test comes to assert
/// against a colour nothing ever painted.
pub fn painted_ink<S>(harness: &Harness<'_, S>, needle: &str) -> Option<Color32> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => Some(paint_ink(text)),
            _ => None,
        })
}

/// Every filled rectangle painted by the last frame as `(rect, fill)`.
///
/// Panel frames, toolbars, rails, tabs, and buttons all emit `Shape::Rect`
/// fills, which makes spec-dimension assertions possible without reaching
/// into egui internals.
pub fn filled_rects<S>(harness: &Harness<'_, S>) -> Vec<(Rect, Color32)> {
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

/// Every stroked rectangle painted by the last frame as `(rect, stroke, width)`.
///
/// [`filled_rects`] reports only shapes with a fill, so a surface defined by
/// its outline — a card border, a divider box — is invisible to it. This is
/// the sibling that sees the stroke itself. A bordered card is then asserted
/// by its signature: one rect present in both lists at the same geometry.
pub fn stroked_rects<S>(harness: &Harness<'_, S>) -> Vec<(Rect, Color32, f32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape)
                if rect_shape.stroke.color != Color32::TRANSPARENT
                    && rect_shape.stroke.width > 0.0 =>
            {
                Some((
                    rect_shape.rect,
                    rect_shape.stroke.color,
                    rect_shape.stroke.width,
                ))
            }
            _ => None,
        })
        .collect()
}

/// Every filled circle painted by the last frame as `(center, radius, fill)`.
///
/// Status dots are painted as circles rather than rects, so [`filled_rects`]
/// cannot see them; this is the sibling that can.
pub fn filled_circles<S>(harness: &Harness<'_, S>) -> Vec<(Pos2, f32, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Circle(circle) if circle.fill != Color32::TRANSPARENT => {
                Some((circle.center, circle.radius, circle.fill))
            }
            _ => None,
        })
        .collect()
}

/// Every stroked path painted by the last frame as `(visual rect, color)`.
///
/// Icons (the Lucide primitives) and focus rings paint as `Shape::Path`,
/// which neither the text queries nor [`filled_rects`] can see. The rect is
/// the shape's visual bounding box (points + half the stroke width), i.e.
/// where the glyph actually appears on screen — which is what "does the
/// interactive widget contain its own glyph" contracts need. Only solid
/// strokes are reported; gradient (`ColorMode::UV`) strokes are skipped.
pub fn painted_paths<S>(harness: &Harness<'_, S>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Path(path) => match &path.stroke.color {
                egui::epaint::ColorMode::Solid(color) => {
                    Some((path.visual_bounding_rect(), *color))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Right-click the Button node whose accessibility label contains `needle`.
///
/// A row's label usually carries more than the text a test names it by — a
/// commit row is `"abc1234 subject"`, a branch row is the bare name — so this
/// matches on containment and lets the caller keep saying what it means.
pub fn right_click_row<S>(harness: &mut Harness<'_, S>, needle: &str) {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|label| label.contains(needle))
        })
        .unwrap_or_else(|| panic!("no row whose label contains {needle}"))
        .click_secondary();
    harness.step();
    harness.step();
}

/// Click one item of the context menu that is open.
///
/// `sentinel` is a label unique to THAT menu, used to find its left edge: the
/// command palette lists its own "Pull", "Push…" and "Checkout" actions
/// elsewhere, and only the menu's rows all share one left edge. A disabled item
/// is still found — the menu's convention is that a blocked action stays
/// visible.
pub fn click_menu_item<S>(harness: &mut Harness<'_, S>, sentinel: &str, label: &str) {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    harness.remove_cursor();
    harness.step();
    let column = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(sentinel.to_string()))
        .unwrap_or_else(|| panic!("the menu is open (no {sentinel:?} item)"))
        .rect();
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label() == Some(label.to_string())
                && (n.rect().min.x - column.min.x).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("menu item {label} inside the open menu"))
        .click();
    harness.step();
}

/// Assert the open menu's item `label` is rendered AND disabled — the whole
/// "a blocked action explains itself" rule, from the accessibility tree rather
/// than from a widget's internals.
#[track_caller]
pub fn assert_menu_item_gated<S>(harness: &mut Harness<'_, S>, sentinel: &str, label: &str) {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    harness.remove_cursor();
    harness.step();
    let column = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(sentinel.to_string()))
        .unwrap_or_else(|| panic!("the menu is open (no {sentinel:?} item)"))
        .rect();
    let item = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label() == Some(label.to_string())
                && (n.rect().min.x - column.min.x).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("menu item {label} inside the open menu"));
    assert!(
        item.accesskit_node().is_disabled(),
        "{label} must be gated, not hidden"
    );
}

/// Step frames until the painted output stabilizes.
///
/// The first frames after startup relayout (embedded fonts take effect at
/// pass 2), so queries and clicks must only happen on a settled frame —
/// mirroring a user clicking an already-rendered shell.
pub fn settle<S>(harness: &mut Harness<'_, S>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("shell layout did not settle within 10 frames");
}

/// Step frames until the painted text is byte-identical for **three consecutive**
/// frames, up to 300, sleeping 10ms between frames so a worker can post between them.
///
/// Kept separate from [`settle`] on purpose: that one answers "has the layout stopped
/// relaying out?" off a single repeat, which a shell with background work reaches
/// *between* worker repaints — so a suite that then clicks acts on stale coordinates.
pub fn settle_quiet<S>(harness: &mut Harness<'_, S>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = fingerprint;
        }
    }
    panic!("shell layout did not settle within 300 frames; last painted:\n{prev}");
}
