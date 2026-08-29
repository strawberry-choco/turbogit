//! Issue #3 — dark-only design tokens and embedded fonts.
//!
//! ADR-0003: `ThemeMode::{Light, HighContrast}` are deleted outright. A
//! legacy `state.ron` carrying a removed theme preference must still load
//! — the stale key is ignored and the app renders the designed dark
//! experience regardless.

use turbogit_domain::model::ProjectState;

/// Complete pre-redesign `state.ron` document whose theme is a removed
/// mode (`HighContrast`; `Light` is exercised via the same shape).
const LEGACY_HIGH_CONTRAST: &str = r#"
(
    mappings: [],
    settings: (
        git_executable: "",
        staging_area: false,
        synchronous_branches: false,
        update_method: Rebase,
        clean_tree_method: Stash,
        incoming_check: Auto,
        protected_branch_patterns: ["main"],
        warn_crlf: true,
        warn_detached: true,
        commit_template: "",
        restore_workspace: false,
        gutter_markers: true,
        date_format: Iso,
        no_commit_hooks: false,
        theme: HighContrast,
    ),
)
"#;

#[test]
fn legacy_state_with_removed_theme_mode_still_loads() {
    let legacy_light = LEGACY_HIGH_CONTRAST.replace("HighContrast", "Light");

    let hc: ProjectState =
        ron::from_str(LEGACY_HIGH_CONTRAST).expect("legacy HighContrast state must load");
    let light: ProjectState = ron::from_str(&legacy_light).expect("legacy Light state must load");

    // The stale theme key is ignored: both legacy documents load to the
    // same clean (dark-only) settings.
    assert_eq!(hc.settings, light.settings);
}

// --- Cycle 2: dark-only Visuals derive from the central token set (spec §2.5) ---

use egui::Color32;
use turbogit_ui::theme::configure_style;

const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
const INK: Color32 = Color32::from_rgb(0xbc, 0xbe, 0xc4);
const INK_2: Color32 = Color32::from_rgb(0xa0, 0xa3, 0xab);
const BRAND: Color32 = Color32::from_rgb(0x35, 0x74, 0xf0);
const STATE_WARNING: Color32 = Color32::from_rgb(0xf9, 0xa8, 0x25);
const STATE_ERROR: Color32 = Color32::from_rgb(0xef, 0x53, 0x50);

#[test]
fn dark_visuals_map_the_spec_tokens() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let v = &ctx.style_of(egui::Theme::Dark).visuals;

    assert_eq!(v.panel_fill, BG, "panel_fill");
    assert_eq!(v.window_fill, SURFACE, "window_fill");
    assert_eq!(v.extreme_bg_color, BG, "extreme_bg_color");
    assert_eq!(v.override_text_color, Some(INK), "override_text_color");
    assert_eq!(v.faint_bg_color, SURFACE_2, "faint_bg_color");
    assert_eq!(v.code_bg_color, SURFACE_3, "code_bg_color");
    assert_eq!(v.hyperlink_color, BRAND, "hyperlink_color");
    assert_eq!(v.warn_fg_color, STATE_WARNING, "warn_fg_color");
    assert_eq!(v.error_fg_color, STATE_ERROR, "error_fg_color");
    assert!(v.dark_mode, "dark-only app must stay in dark mode");
}

#[test]
fn widget_states_follow_the_surface_mapping() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let w = &ctx.style_of(egui::Theme::Dark).visuals.widgets;

    assert_eq!(w.noninteractive.bg_fill, SURFACE, "noninteractive.bg_fill");
    assert_eq!(w.noninteractive.fg_stroke.color, INK_2, "noninteractive.fg");
    assert_eq!(w.inactive.bg_fill, SURFACE_2, "inactive.bg_fill");
    assert_eq!(w.inactive.fg_stroke.color, INK_2, "inactive.fg");
    assert_eq!(w.hovered.bg_fill, SURFACE_2, "hovered.bg_fill");
    assert_eq!(w.hovered.fg_stroke.color, INK, "hovered.fg");
    assert_eq!(w.active.bg_fill, SURFACE_3, "active.bg_fill");
    assert_eq!(w.active.fg_stroke.color, INK, "active.fg");
    assert_eq!(w.open.bg_fill, SURFACE_3, "open.bg_fill");
    assert_eq!(w.open.fg_stroke.color, INK, "open.fg");
}

#[test]
fn selection_uses_brand_with_brand_stroke() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let sel = &ctx.style_of(egui::Theme::Dark).visuals.selection;

    assert_eq!(sel.stroke.color, BRAND, "selection.stroke");
    assert_eq!(sel.stroke.width, 1.0);
    // BRAND at ~25% premultiplied alpha: 0.25 * channel.
    assert_eq!(
        sel.bg_fill,
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40),
        "selection.bg_fill"
    );
}

// --- Cycle 3: embedded JetBrains Mono with system fallbacks (ADR-0002, spec §3.1) ---

use turbogit_ui::theme::font_definitions;

#[test]
fn proportional_family_is_jetbrains_mono_with_segoe_ui_fallback() {
    let defs = font_definitions();
    let fam = &defs.families.get(&egui::FontFamily::Proportional).unwrap();
    assert_eq!(fam[0], "jetbrains-mono-regular", "primary UI font");
    // The Segoe UI fallback is registered only when the face is actually
    // present on this machine (Windows); it degrades gracefully by
    // omission elsewhere (ADR-0002).
    if cfg!(windows) {
        assert!(
            fam.iter().any(|f| f == "Segoe UI"),
            "Segoe UI fallback required"
        );
    } else {
        assert!(
            !fam.iter().any(|f| f == "Segoe UI"),
            "absent system face must be omitted"
        );
    }
}

#[test]
fn monospace_family_is_jetbrains_mono_with_consolas_fallback() {
    let defs = font_definitions();
    let fam = &defs.families.get(&egui::FontFamily::Monospace).unwrap();
    assert_eq!(fam[0], "jetbrains-mono-regular", "primary mono font");
    if cfg!(windows) {
        assert!(
            fam.iter().any(|f| f == "Consolas"),
            "Consolas fallback required"
        );
    } else {
        assert!(
            !fam.iter().any(|f| f == "Consolas"),
            "absent system face must be omitted"
        );
    }
}

#[test]
fn bold_weight_is_embedded_as_its_own_family() {
    let defs = font_definitions();
    let bold_family = egui::FontFamily::Name("jetbrains-mono-bold".into());
    let fam = &defs
        .families
        .get(&bold_family)
        .expect("bold family registered");
    assert_eq!(fam[0], "jetbrains-mono-bold");

    // The bold weight is really embedded (non-empty TrueType data).
    let data = &defs
        .font_data
        .get("jetbrains-mono-bold")
        .expect("bold font data");
    assert!(data.font.len() > 100_000, "bold ttf should be ~160KB");
}

#[test]
fn embedded_font_data_is_valid_truetype() {
    let defs = font_definitions();
    for key in ["jetbrains-mono-regular", "jetbrains-mono-bold"] {
        let data = defs
            .font_data
            .get(key)
            .unwrap_or_else(|| panic!("{key} missing"));
        // sfnt magic: 0x00010000 (TrueType) or 'OTTO' (CFF).
        let magic = &data.font[0..4];
        assert!(
            magic == [0x00, 0x01, 0x00, 0x00] || magic == b"OTTO",
            "{key} is not a valid sfnt font"
        );
    }
}

// --- Cycle 4: the app installs the stack into its context (ADR-0002) ---

use turbogit_ui::theme::install_fonts;

#[test]
fn install_fonts_applies_the_embedded_stack_to_the_context() {
    let ctx = egui::Context::default();
    install_fonts(&ctx);

    // Font definitions take effect at the next pass begin.
    let mut full = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    full.textures_delta.clear();

    ctx.fonts(|f| {
        let defs = f.definitions();
        let fam = defs
            .families
            .get(&egui::FontFamily::Proportional)
            .expect("proportional family registered");
        assert_eq!(fam[0], "jetbrains-mono-regular", "UI text renders in JBM");
        let bold = egui::FontFamily::Name("jetbrains-mono-bold".into());
        assert!(defs.families.contains_key(&bold), "bold family available");
    });
}
