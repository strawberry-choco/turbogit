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

use egui::{Color32, FontFamily, Pos2, Rect, Shape};
use egui_kittest::Harness;
use turbogit_app::state::AppState;
use turbogit_ui::theme::{configure_style, install_fonts};

/// A harness rendering the full shell over a fresh [`AppState`].
///
/// The project dir is an empty temp directory: zero roots are discovered, so
/// the render is deterministic, no background git workers are spawned, and —
/// per the Welcome-vs-shell model (spec §9.2) — the central body shows the
/// Welcome placeholder while every shell region still renders.
/// Setup mirrors production (`app.rs`): dark-only tokens every frame plus
/// embedded JetBrains Mono installed once.
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

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    (harness, project)
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
    /// `RichText::color` / the widget's ink, from the galley's layout job.
    pub color: Color32,
    /// The resolved face — `Proportional` for chrome, `Monospace` for data
    /// (branch and remote names).
    pub family: FontFamily,
}

/// Every text galley painted by the last frame.
///
/// The single primitive behind the label queries above. A string usually paints
/// in more than one place — the current branch names both a topbar button and a
/// row; a repo name appears in the sidebar and in a section header — so ask for
/// the color or face of *this* occurrence by matching on `pos`, rather than
/// trusting the order shapes happen to arrive in. Color and font both land in
/// the galley's layout job, so a token-colored, face-correct label (the active
/// branch's soft blue monospace, a diverged branch's red) is assertable from
/// painted output alone — no reach into widget internals.
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
                    color: section
                        .map(|s| s.format.color)
                        .unwrap_or(Color32::TRANSPARENT),
                    family: section
                        .map(|s| s.format.font_id.family.clone())
                        .unwrap_or(FontFamily::Proportional),
                })
            }
            _ => None,
        })
        .collect()
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
