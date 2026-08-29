//! Issue #7 — Seam: the public `ui::icons` API asserted through painted
//! shapes from a headless egui context (no GPU / window / display server).
//! Mirrors the painted-output style of `shell_frame.rs` at module scope.
use std::sync::Mutex;

use egui::epaint::ColorMode;
use egui::{Color32, Pos2, Shape};
use turbogit_ui::theme::{Palette, icon_color};
use turbogit_ui::ui::icons;

/// Run one headless frame painting via `paint`; return all painted shapes.
fn painted_shapes(paint: impl Fn(&mut egui::Ui)) -> Vec<Shape> {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(200.0, 200.0),
        )),
        ..Default::default()
    };
    let mut full = ctx.run_ui(raw, |ui| paint(ui));
    full.textures_delta.clear();
    full.shapes
        .into_iter()
        .map(|clipped| clipped.shape)
        .collect()
}

/// Stroked path primitives (what icon rendering emits).
fn stroked_paths(shapes: &[Shape]) -> Vec<&egui::epaint::PathShape> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.fill == Color32::TRANSPARENT => Some(p),
            _ => None,
        })
        .collect()
}

/// The solid stroke color of a path, if any.
fn stroke_color(p: &egui::epaint::PathShape) -> Option<Color32> {
    match p.stroke.color {
        ColorMode::Solid(c) => Some(c),
        _ => None,
    }
}

// --- Cycle 1: a known name renders vector primitives at size & color ---

#[test]
fn known_icon_paints_strokes_at_requested_size_and_color() {
    let shapes = painted_shapes(|ui| {
        icons::icon(ui, icons::Icon::CHECK, 24.0, Palette::BRAND);
    });
    let paths = stroked_paths(&shapes);
    assert!(!paths.is_empty(), "check must paint stroked primitives");

    for p in &paths {
        assert_eq!(
            stroke_color(p),
            Some(Palette::BRAND),
            "strokes must carry the requested tint"
        );
        assert!(
            (p.stroke.width - 2.0).abs() < 1e-3,
            "2px Lucide stroke at scale 24/24"
        );
    }

    // Geometry fits the requested box (extent ≤ size + stroke tolerance).
    let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in &paths {
        for pt in &p.points {
            min = min.min(*pt);
            max = max.max(*pt);
        }
    }
    assert!(max.x - min.x <= 26.0 && max.y - min.y <= 26.0);
}

#[test]
fn icon_tint_derives_from_central_palette_tokens() {
    let shapes = painted_shapes(|ui| {
        icons::icon(ui, icons::Icon::CHECK, 16.0, icon_color());
    });
    let paths = stroked_paths(&shapes);
    assert!(!paths.is_empty(), "icon must paint via theme::icon_color()");
    for p in &paths {
        assert_eq!(
            stroke_color(p),
            Some(Palette::INK_2),
            "tint must be the central palette token"
        );
        assert!((p.stroke.width - 2.0 * (16.0 / 24.0)).abs() < 1e-3);
    }
}

// --- Cycle 2: the complete documented set renders ---

/// The exact icon names from spec §5.2 — the contract this module implements.
const SPEC_NAMES: [&str; 56] = [
    "alert-circle",
    "alert-triangle",
    "align-justify",
    "archive",
    "arrow-down",
    "arrow-down-circle",
    "arrow-left",
    "arrow-right",
    "arrow-right-left",
    "arrow-up",
    "bell",
    "book-open",
    "bug",
    "check",
    "check-square",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "chevron-up",
    "clock",
    "columns",
    "download",
    "eye-off",
    "file",
    "file-code",
    "file-minus",
    "file-plus",
    "file-warning",
    "files",
    "filter",
    "folder",
    "folder-git",
    "folder-open",
    "git-branch",
    "git-commit",
    "git-compare",
    "git-merge",
    "keyboard",
    "laptop",
    "layers",
    "layout",
    "menu",
    "monitor",
    "more-horizontal",
    "play",
    "plus",
    "plus-circle",
    "refresh-cw",
    "search",
    "settings",
    "star",
    "tag",
    "trash-2",
    "undo",
    "upload",
    "x",
];

#[test]
fn every_documented_icon_name_renders_primitives() {
    // The embedded set is exactly the documented set.
    assert_eq!(
        icons::Icon::ALL.len(),
        SPEC_NAMES.len(),
        "embedded icon count must match spec §5.2"
    );

    for name in SPEC_NAMES {
        let icon = icons::Icon::from_name(name)
            .unwrap_or_else(|| panic!("`{name}` missing from the embedded set"));
        assert_eq!(icon.name(), name);

        let shapes = painted_shapes(|ui| icons::icon(ui, icon, 24.0, Palette::BRAND));
        let paths = stroked_paths(&shapes);
        assert!(!paths.is_empty(), "`{name}` painted nothing");

        let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in &paths {
            assert_eq!(stroke_color(p), Some(Palette::BRAND));
            for pt in &p.points {
                assert!(
                    pt.is_finite(),
                    "`{name}` produced non-finite geometry at {pt:?}"
                );
                min = min.min(*pt);
                max = max.max(*pt);
            }
        }
        assert!(
            max.x - min.x <= 26.0 && max.y - min.y <= 26.0,
            "`{name}` overflows its box: {min:?}..{max:?}"
        );
    }
}

#[test]
fn lookup_by_name_round_trips_every_icon() {
    for icon in icons::Icon::ALL {
        assert_eq!(icons::Icon::from_name(icon.name()), Some(*icon));
    }
    assert_eq!(icons::Icon::from_name("check"), Some(icons::Icon::CHECK));
}

// --- Cycle 3: unknown names are safe misses ---

mod log_capture {
    use std::sync::{Mutex, OnceLock};

    /// Captured `(level, message)` records, shared process-wide.
    pub static RECORDS: OnceLock<Mutex<Vec<(log::Level, String)>>> = OnceLock::new();

    pub struct Capture;

    impl log::Log for Capture {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata())
                && let Some(records) = RECORDS.get()
            {
                records
                    .lock()
                    .expect("log capture mutex")
                    .push((record.level(), record.args().to_string()));
            }
        }

        fn flush(&self) {}
    }
}

#[test]
fn unknown_icon_name_logs_debug_and_draws_nothing() {
    // Install once per process; a competing install is fine to lose.
    let _ = log::set_boxed_logger(Box::new(log_capture::Capture));
    log::set_max_level(log::LevelFilter::Debug);
    let records = log_capture::RECORDS.get_or_init(|| Mutex::new(Vec::new()));

    let shapes = painted_shapes(|ui| {
        icons::icon_by_name(ui, "not-an-icon", 16.0, Palette::BRAND);
        icons::icon_by_name(ui, "", 16.0, Palette::BRAND);
    });
    assert!(
        stroked_paths(&shapes).is_empty(),
        "unknown names must draw nothing"
    );

    let captured = records.lock().expect("log capture mutex");
    assert!(
        captured
            .iter()
            .any(|(level, msg)| *level == log::Level::Debug && msg.contains("not-an-icon")),
        "`not-an-icon` miss must be logged at debug level; got {captured:?}"
    );
    let misses = captured
        .iter()
        .filter(|(level, msg)| *level == log::Level::Debug && msg.contains("unknown icon"))
        .count();
    assert!(
        misses >= 2,
        "both unknown-name calls must be reported as misses; got {misses}"
    );
}
