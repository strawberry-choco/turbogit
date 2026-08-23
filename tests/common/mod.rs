//! Reusable headless-shell harness helpers (issue #9).
//!
//! Shared by `redesign_harness.rs` and later redesign test files so each
//! ticket can drive the full [`turbogit::ui::render`] shell without copying
//! setup code. The harness runs over synthetic raw input (egui_kittest, no
//! GPU / window / display server) and asserts only on public surfaces:
//!
//! - **Painted output** — the frame's shapes from `FullOutput`, i.e. exactly
//!   what a software painter would fill (text galleys carry their strings;
//!   filled rects carry their geometry + token color).
//! - **State transitions** — public `AppState` fields after the frames.

// Each integration-test crate compiles its own copy of this module and uses
// only the helpers it needs; the rest are intentionally unused there.
#![allow(dead_code)]

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::Harness;
use turbogit::state::AppState;
use turbogit::theme::{configure_style, install_fonts};

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
    let state = AppState::new(project.path().to_path_buf());
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
            turbogit::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    (harness, project)
}

/// All text painted by the last completed frame.
pub fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
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
pub fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Assert `needle` appears in no painted text galley.
#[track_caller]
pub fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
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
pub fn galley_origin(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
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
///
/// Panel frames, toolbars, rails, tabs, and buttons all emit `Shape::Rect`
/// fills, which makes spec-dimension assertions possible without reaching
/// into egui internals.
pub fn filled_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
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
///
/// The first frames after startup relayout (embedded fonts take effect at
/// pass 2), so queries and clicks must only happen on a settled frame —
/// mirroring a user clicking an already-rendered shell.
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
    panic!("shell layout did not settle within 10 frames");
}
