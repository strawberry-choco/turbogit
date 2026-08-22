//! Issue #4 — Seam 2: headless egui harness driving `ui::render()`.
//!
//! The harness runs the top-level [`turbogit::ui::render`] end-to-end over
//! synthetic raw input (egui_kittest, no GPU / window / display server) and
//! asserts only on public surfaces:
//!
//! - **Painted output** — the frame's shapes from `FullOutput`, i.e. exactly
//!   what a software painter would fill (text galleys carry their strings).
//! - **State transitions** — public `AppState` fields after the frames.
//!
//! Later redesign tickets assert navigation, inert controls, dialog cycles,
//! placeholder panes, and shortcut dispatch against this harness.

use egui::{Key, Modifiers, Shape};
use egui_kittest::{kittest::Queryable, Harness};
use turbogit::state::{AppState, Dialog, Tab};
use turbogit::theme::{configure_style, install_fonts};

/// A harness rendering the full shell over a fresh [`AppState`].
///
/// The project dir is an empty temp directory: zero roots are discovered, so
/// the render is deterministic and no background git workers are spawned.
/// Setup mirrors production (`app.rs`): dark-only tokens every frame plus
/// embedded JetBrains Mono installed once.
fn shell_harness() -> (Harness<'static, AppState>, tempfile::TempDir) {
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

/// Assert `needle` appears in some painted text galley.
#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Step frames until the painted output stabilizes.
///
/// The first frames after startup relayout (embedded fonts take effect at
/// pass 2), so queries and clicks must only happen on a settled frame —
/// mirroring a user clicking an already-rendered shell.
fn settle(harness: &mut Harness<'_, AppState>) {
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

// --- Cycle 1: initial render paints the shell over an empty project ---

#[test]
fn initial_render_paints_the_shell_over_an_empty_project() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    for label in [
        "Repositories",           // left pane heading
        "No repository selected", // status bar with zero roots
        "Commit",                 // central tabs
        "Log",
        "History",
        "⏏ Push", // status bar actions
        "⤓ Pull",
        "⚙ Settings",
    ] {
        assert_painted(&harness, label);
    }
}

// --- Cycle 2: synthetic keyboard input drives a navigation transition ---

#[test]
fn ctrl_shift_k_opens_the_push_dialog() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness); // shell without any dialog

    assert_eq!(harness.state().ui.dialog, None);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::Push),
        "Ctrl+Shift+K must open the Push dialog"
    );
    // The dialog chrome is really painted, not just state.
    assert_painted(&harness, "Remote:");
    assert_painted(&harness, "Branch:");
    assert_painted(&harness, "Force push (--force-with-lease)");
}

// --- Cycle 3: synthetic mouse input through the accessibility tree ---

#[test]
fn clicking_the_log_tab_switches_tool_window() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.get_by_label("Log").click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.tab,
        Tab::Log,
        "clicking the Log tab must switch tool windows"
    );
}
