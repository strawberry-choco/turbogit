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
use turbogit_ui::theme::{self, Palette, configure_style};

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

// --- Cycle 5: accent, risk, and status semantics (issue #01) ---
#[test]
fn accent_accessor_returns_brand() {
    assert_eq!(theme::accent(), Palette::BRAND, "accent() must alias BRAND");
}

#[test]
fn risk_tokens_are_distinct_and_in_the_status_family() {
    // Risk levels use the three state colors in increasing severity order.
    assert_eq!(Palette::RISK_LOW, Palette::STATE_SUCCESS);
    assert_eq!(Palette::RISK_MEDIUM, Palette::STATE_WARNING);
    assert_eq!(Palette::RISK_HIGH, Palette::STATE_ERROR);

    // All three risk tokens must be distinct — a status surface never
    // collapses to a single color regardless of severity.
    assert_ne!(Palette::RISK_LOW, Palette::RISK_MEDIUM);
    assert_ne!(Palette::RISK_MEDIUM, Palette::RISK_HIGH);
    assert_ne!(Palette::RISK_LOW, Palette::RISK_HIGH);
}

#[test]
fn status_tokens_cover_clean_dirty_diverged_stale() {
    // The four status semantics each pick a state token (clean = success,
    // dirty = warning, diverged = error, stale = info) so a chip carrying
    // any of them has a real color, never a fallback.
    assert_eq!(Palette::STATUS_CLEAN, Palette::STATE_SUCCESS);
    assert_eq!(Palette::STATUS_DIRTY, Palette::STATE_WARNING);
    assert_eq!(Palette::STATUS_DIVERGED, Palette::STATE_ERROR);
    assert_eq!(Palette::STATUS_STALE, Palette::STATE_INFO);

    for pair in [
        (Palette::STATUS_CLEAN, Palette::STATUS_DIRTY),
        (Palette::STATUS_DIRTY, Palette::STATUS_DIVERGED),
        (Palette::STATUS_DIVERGED, Palette::STATUS_STALE),
        (Palette::STATUS_CLEAN, Palette::STATUS_DIVERGED),
    ] {
        assert_ne!(
            pair.0, pair.1,
            "status tokens {:?} and {:?} must differ",
            pair.0, pair.1
        );
    }
}

// --- Cycle 6: Local Changes redesign token set (issue 01, ticket 01) ---
//
// Purely additive: every token below is new and nothing existing re-renders
// differently — later tickets opt in by switching call sites. Sources:
// `docs/ui-local-changes-visual-changes.md` §7–8 and the redesign mockup SVG.

const SIDEBAR: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1e);

#[test]
fn sidebar_token_is_dedicated_and_distinct_from_shell_surfaces() {
    // The redesign gives the sidebar rail its own surface (#1B1C1E), darker
    // than both the window fill (BG) and panel fills (SURFACE*).
    assert_eq!(
        Palette::SIDEBAR,
        SIDEBAR,
        "sidebar surface must match the design"
    );
    for shell_surface in [
        Palette::BG,
        Palette::SURFACE,
        Palette::SURFACE_2,
        Palette::SURFACE_3,
    ] {
        assert_ne!(
            Palette::SIDEBAR,
            shell_surface,
            "sidebar must be a distinct surface token, not an alias of {shell_surface:?}"
        );
    }
    // The window/panel tokens themselves stay untouched by the redesign
    // (they already matched the design doc §7 surfaces).
    assert_eq!(Palette::BG, Color32::from_rgb(0x1e, 0x1f, 0x22));
    assert_eq!(Palette::SURFACE, Color32::from_rgb(0x2b, 0x2d, 0x30));
}

const SELECTION_BG: Color32 = Color32::from_rgb(0x2e, 0x43, 0x6e);

#[test]
fn selection_background_token_is_solid_design_blue_and_add_only() {
    // The redesign's row-selection background is the solid #2E436E the Local
    // Changes file rows (ticket 05) opt into — a new token, not a mutation of
    // the translucent selection_bg() the shell rows paint today.
    assert_eq!(
        Palette::SELECTION_BG,
        SELECTION_BG,
        "selection bg must match the design"
    );
    assert_eq!(
        Palette::SELECTION_BG,
        Color32::from_rgb(0x2e, 0x43, 0x6e),
        "selection bg is fully opaque"
    );

    // Purely additive: the existing translucent selection fill is untouched,
    // so no widget that renders selection today changes color.
    let opaque = Palette::SELECTION_BG.is_opaque();
    assert!(
        opaque,
        "redesign selection bg must be solid, not alpha-blended"
    );
    assert_ne!(
        Palette::SELECTION_BG,
        Palette::selection_bg(),
        "new solid token must not alias the legacy translucent fill"
    );
    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40),
        "legacy selection_bg() must remain the translucent brand blend"
    );
}

#[test]
fn status_letter_tokens_match_the_mockup_modified_added_unversioned() {
    // File rows (issue 05) colour the status letter and the filename by state:
    // M modified blue, A added green, U unversioned olive — values straight
    // from the redesign mockup.
    assert_eq!(
        Palette::STATUS_MODIFIED,
        Color32::from_rgb(0xa8, 0xc0, 0xe8),
        "M must render in the mockup's modified blue"
    );
    assert_eq!(
        Palette::STATUS_ADDED,
        Color32::from_rgb(0x57, 0x96, 0x5c),
        "A must render in the mockup's added green"
    );
    assert_eq!(
        Palette::STATUS_UNVERSIONED,
        Color32::from_rgb(0xb5, 0xb3, 0x7e),
        "U must render in the mockup's unversioned olive"
    );

    // The three states are pairwise distinct so a row is never ambiguous
    // about which status letter it carries.
    let letters = [
        Palette::STATUS_MODIFIED,
        Palette::STATUS_ADDED,
        Palette::STATUS_UNVERSIONED,
    ];
    for (i, a) in letters.iter().enumerate() {
        for b in letters.iter().skip(i + 1) {
            assert_ne!(a, b, "status-letter tokens must be pairwise distinct");
        }
    }
}

#[test]
fn redesign_diff_tokens_match_the_mockup_and_stay_distinct_from_legacy() {
    // The diff preview (issue 06) paints added/deleted lines as accent color
    // on a tinted block pair — values straight from the design doc §7.
    assert_eq!(
        Palette::DIFF_ADD_ACCENT,
        Color32::from_rgb(0x57, 0x96, 0x5c),
        "added accent"
    );
    assert_eq!(
        Palette::DIFF_ADD_BLOCK,
        Color32::from_rgb(0x2e, 0x43, 0x34),
        "added block"
    );
    assert_eq!(
        Palette::DIFF_DEL_ACCENT,
        Color32::from_rgb(0xf7, 0x54, 0x64),
        "removed accent"
    );
    assert_eq!(
        Palette::DIFF_DEL_BLOCK,
        Color32::from_rgb(0x43, 0x30, 0x34),
        "removed block"
    );

    // Added shares its green with the added-file status letter — one hue for
    // 'added' across rows and diff.
    assert_eq!(Palette::DIFF_ADD_ACCENT, Palette::STATUS_ADDED);
    // Accent and block of each pair are distinct; the two pairs never cross.
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BLOCK);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BLOCK);
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_DEL_ACCENT);
    assert_ne!(Palette::DIFF_ADD_BLOCK, Palette::DIFF_DEL_BLOCK);

    // Purely additive: the legacy diff view tokens are untouched — the
    // redesign pair is a separate token set, not a re-paint of DIFF_*_BG/TEXT.
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_TEXT);
    assert_ne!(Palette::DIFF_ADD_BLOCK, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_TEXT);
    assert_ne!(Palette::DIFF_DEL_BLOCK, Palette::DIFF_DEL_BG);
}

#[test]
fn counter_token_is_reserved_orange_distinct_from_every_accent() {
    // #E0883C paints dirt/unpulled counters only (design doc §7) — it is
    // semantically reserved, never intended as a general accent or warning.
    assert_eq!(
        Palette::COUNTER,
        Color32::from_rgb(0xe0, 0x88, 0x3c),
        "counter orange must match the design"
    );
    assert!(
        Palette::COUNTER.is_opaque(),
        "counter orange is a solid fill"
    );

    // Reserved: it must not alias the brand accent, the generic warning
    // orange, or the error red — so a counter can never be confused with an
    // action, a warning state, or a diverged state.
    assert_ne!(Palette::COUNTER, Palette::BRAND, "not the general accent");
    assert_ne!(
        Palette::COUNTER,
        Palette::STATE_WARNING,
        "not the generic warning"
    );
    assert_ne!(Palette::COUNTER, Palette::STATE_ERROR, "not the error red");
}

#[test]
fn spacing_scale_tokens_match_the_redesign_spec() {
    // Design doc §8: one consistent row height per row kind, gaps in
    // multiples of 4, panel padding 12–14 px.
    assert_eq!(theme::FILE_ROW_HEIGHT, 24.0, "file-row height");
    assert_eq!(theme::GROUP_ROW_HEIGHT, 26.0, "group-row height");

    assert_eq!(theme::GRID_GAP, 4.0, "grid gap base unit");
    assert_eq!(theme::PANEL_PADDING, 12.0, "panel padding (12–14 px scale)");
    // The padding is on the multiples-of-4 grid the design mandates.
    assert_eq!(
        theme::PANEL_PADDING as i32 % theme::GRID_GAP as i32,
        0,
        "panel padding sits on the 4 px grid"
    );
}
