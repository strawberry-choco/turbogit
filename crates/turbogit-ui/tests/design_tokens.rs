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

#[test]
fn p0_shell_defaults_use_the_branch_type_ramp() {
    use egui::TextStyle;
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let style = ctx.style_of(egui::Theme::Dark);
    for (role, font) in [
        (TextStyle::Body, theme::chrome_font(theme::TYPE_BODY)),
        (TextStyle::Button, theme::chrome_font(theme::TYPE_CONTROL)),
        (TextStyle::Small, theme::chrome_font(theme::TYPE_CONTROL)),
        (
            TextStyle::Heading,
            theme::chrome_font(theme::TYPE_DETAIL_TITLE),
        ),
        (TextStyle::Monospace, theme::data_font(theme::TYPE_BODY)),
    ] {
        assert_eq!(style.text_styles[&role], font, "{role:?}");
    }
}

#[test]
fn p0_muted_and_accent_ink_meet_aa_on_audited_surfaces() {
    fn luminance(color: Color32) -> f64 {
        let linear = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    for (ink_name, ink) in [
        ("muted", Palette::T_MUTED),
        ("secondary", Palette::T_SECONDARY),
        ("accent ink", Palette::ACCENT_TEXT),
    ] {
        for (surface_name, surface) in [
            ("content", Palette::CONTENT_BG),
            ("surface", Palette::SURFACE),
            ("sidebar", Palette::SIDEBAR),
            ("panel", Palette::PANEL_BG),
            ("hover", Palette::SURFACE_2),
            ("raised", Palette::SURFACE_3),
            ("selection", Palette::SELECTION),
            ("selection bg", Palette::SELECTION_BG),
        ] {
            let ratio = (luminance(ink) + 0.05) / (luminance(surface) + 0.05);
            println!("{ink_name} on {surface_name}: {ratio:.3}:1");
            assert!(ratio >= 4.5, "{ink_name} on {surface_name}: {ratio:.3}:1");
        }
    }
}

#[test]
fn p0_repo_state_shared_mapping_is_app_wide() {
    use turbogit_ui::theme::RepoState;
    // One status→colour map: dots, smart groups, badges and headers agree.
    let expected = [
        (RepoState::Clean, Palette::AHEAD),
        (RepoState::Unpushed, Palette::AHEAD),
        (RepoState::Dirty, Palette::COUNTER),
        (RepoState::Unpulled, Palette::COUNTER),
        (RepoState::Conflict, Palette::STATUS_DIVERGED),
        (RepoState::Diverged, Palette::STATUS_DIVERGED),
    ];
    for (state, color) in expected {
        assert_eq!(state.color(), color, "{state:?}");
    }
    assert_eq!(
        turbogit_ui::ui::sidebar::dot_color(RepoState::Dirty),
        Palette::COUNTER
    );
    assert_eq!(
        turbogit_ui::ui::sidebar::dot_color(RepoState::Diverged),
        Palette::STATUS_DIVERGED
    );
    // Sync badge vocabulary matches the same map.
    assert_eq!(
        turbogit_ui::ui::components::sync_ink(turbogit_ui::ui::components::SyncKind::Diverged),
        RepoState::Diverged.color()
    );
    assert_eq!(
        turbogit_ui::ui::components::sync_ink(turbogit_ui::ui::components::SyncKind::Behind),
        RepoState::Unpulled.color()
    );
    assert_eq!(
        turbogit_ui::ui::components::sync_ink(turbogit_ui::ui::components::SyncKind::Ahead),
        RepoState::Unpushed.color()
    );
    // Status-bar counters use the same family (diverged red, dirty/unpulled orange).
    assert_eq!(RepoState::Diverged.color(), Palette::STATUS_DIVERGED);
    assert_eq!(RepoState::Unpulled.color(), Palette::COUNTER);
}

// --- C3/C4: one authoritative text hierarchy and selection model (ticket 04) ---

#[test]
fn one_authoritative_primary_ink_across_shell_and_tool_windows() {
    // C3: the shell default text and the §13 body text are ONE primary role —
    // no competing INK-vs-T_PRIMARY definitions for the same role.
    assert_eq!(
        Palette::INK,
        Palette::T_PRIMARY,
        "IK must be an alias of the authoritative primary"
    );
    // The secondary/muted levels also keep their single-role aliases.
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);

    // The three levels are distinct tokens so hierarchy is never accidental.
    assert_ne!(Palette::INK, Palette::T_SECONDARY);
    assert_ne!(Palette::T_SECONDARY, Palette::T_MUTED);
}

#[test]
fn secondary_and_muted_keep_readable_distinct_roles() {
    // The hierarchy is explicit while preserving the audited 4.5:1 pairings:
    // the muted level stays usable on every audited surface (never dimmed to
    // manufacture a hierarchy step).
    let ctx = egui::Context::default();
    configure_style(&ctx);

    // Both levels still clear 4.5:1 on the darkest audited surface
    // (SELECTION, whose commit-inclusion alias shares this value).
    let luminance = |c: Color32| {
        let lin = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    for (ink_name, ink) in [
        ("secondary", Palette::T_SECONDARY),
        ("muted", Palette::T_MUTED),
    ] {
        let ratio = (luminance(ink) + 0.05) / (luminance(Palette::SELECTION) + 0.05);
        assert!(
            ratio >= 4.5,
            "{ink_name} on selection must stay ≥4.5:1, got {ratio:.3}:1"
        );
    }
}

#[test]
fn selection_uses_one_canonical_opaque_fill() {
    // C4: commit inclusion and navigation selection share ONE opaque fill —
    // the two near-duplicate literals (#2E4369 / #2E436E) collapse into the
    // canonical SELECTION token. The translucent focus fill stays separate
    // and is documented against its compositing background (ticket 03).
    assert_eq!(
        Palette::SELECTION_BG,
        Palette::SELECTION,
        "inclusion reuses the canonical opaque selection fill"
    );
    assert!(Palette::SELECTION_BG.is_opaque());
    // The translucent focus treatment is a distinct, documented variant.
    assert_ne!(
        Palette::SELECTION_BG,
        Palette::selection_bg(),
        "opaque selection must stay distinct from the translucent focus fill"
    );
}

// --- Cycle 2: dark-only Visuals derive from the central token set (spec §2.5) ---
use egui::Color32;
use turbogit_ui::theme::{self, Palette, configure_style};

const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
const INK: Color32 = Color32::from_rgb(0xdf, 0xe1, 0xe5); // unified primary (C3)
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
    assert_eq!(
        v.hyperlink_color,
        Palette::ACCENT_TEXT,
        "hyperlink_color (B2: readable accent ink)"
    );
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
    assert_eq!(
        w.noninteractive.fg_stroke.color,
        Palette::T_SECONDARY,
        "noninteractive.fg (B2 lift)"
    );
    assert_eq!(w.inactive.bg_fill, SURFACE_2, "inactive.bg_fill");
    assert_eq!(
        w.inactive.fg_stroke.color,
        Palette::T_SECONDARY,
        "inactive.fg (B2 lift)"
    );
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

#[test]
fn selection_background_token_is_solid_and_aliases_canonical_selection() {
    // C4: the commit-inclusion fill is the canonical opaque SELECTION token
    // (their previously near-duplicate #2E4369/#2E436E values are unified),
    // and it never aliases the translucent shell focus fill.
    assert_eq!(
        Palette::SELECTION_BG,
        Palette::SELECTION,
        "inclusion reuses the canonical opaque selection fill"
    );
    assert!(Palette::SELECTION_BG.is_opaque(), "selection fill is solid");

    // The translucent focus treatment stays a distinct, documented variant
    // composited over the surface it sits on (ticket 03).
    assert_ne!(
        Palette::SELECTION_BG,
        Palette::selection_bg(),
        "solid selection must not alias the translucent focus fill"
    );
    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40),
        "selection_bg() must remain the translucent brand blend"
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

// --- S1/S2: named density and spacing roles (ticket 06) ---------------------

#[test]
fn shared_spacing_roles_match_the_configured_defaults() {
    // S1: the shell's configured defaults are the shared roles — one source
    // for item gaps, window margins, control padding and indentation; the
    // false "every gap is a multiple of 4" claim is superseded here.
    assert_eq!(theme::ITEM_SPACING, egui::vec2(8.0, 6.0));
    assert_eq!(theme::WINDOW_MARGIN, 10);
    assert_eq!(theme::BUTTON_PADDING, egui::vec2(10.0, 5.0));
    assert_eq!(theme::INDENT, 14.0);

    let ctx = egui::Context::default();
    configure_style(&ctx);
    let spacing = &ctx.style_of(egui::Theme::Dark).spacing;
    assert_eq!(spacing.item_spacing, theme::ITEM_SPACING);
    assert_eq!(
        spacing.window_margin,
        egui::Margin::same(theme::WINDOW_MARGIN)
    );
    assert_eq!(spacing.button_padding, theme::BUTTON_PADDING);
    assert_eq!(spacing.indent, theme::INDENT);
}

#[test]
fn named_density_variants_cover_shell_and_status_bar() {
    // S2: compact shell controls and the dense status bar are named roles,
    // not regional literals — central defaults plus explicit exceptions.
    assert_eq!(theme::DENSITY_COMPACT_BUTTON, egui::vec2(8.0, 4.0));
    assert_eq!(theme::DENSITY_DENSE_BUTTON, egui::vec2(6.0, 2.0));
    // Compact/dense are genuinely tighter than the default control padding
    // (static property — the values are compile-time constants).
    const { assert!(theme::DENSITY_COMPACT_BUTTON.y < theme::BUTTON_PADDING.y) };
    const { assert!(theme::DENSITY_DENSE_BUTTON.y < theme::DENSITY_COMPACT_BUTTON.y) };
    // The default window radius stays distinct from chip/control radii
    // (S3 guards chip shapes, not window chrome).
    assert_ne!(theme::WINDOW_RADIUS, theme::CHIP_RADIUS);
}

#[test]
fn chip_radii_are_shared_variants_not_local_contracts() {
    // S3: compact chips and the welcome pill are explicit shared shape
    // variants — the pill radius equals the welcome chip's half-height, and
    // chip/control radii stay distinct from window/menu surface roles.
    assert_eq!(theme::CHIP_RADIUS, 3);
    assert_eq!(theme::CONTROL_RADIUS, 4);
    assert_eq!(theme::PILL_RADIUS, 9);
    assert_ne!(theme::CHIP_RADIUS, theme::PILL_RADIUS);
    assert_ne!(theme::CONTROL_RADIUS, theme::PILL_RADIUS);
}

// --- G1: the consolidated role contract agrees with its consumers (ticket 07) --

#[test]
fn governance_consolidated_role_contract_is_internally_consistent() {
    // One pass over the whole reconciled contract (docs/design-system-roles.md):
    // each shared role resolves through exactly one authoritative source and
    // stays distinct where semantics differ.
    // C3 — one primary ink, two distinct lower levels.
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_ne!(Palette::T_SECONDARY, Palette::T_MUTED);
    // C4 — one opaque selection fill; translucent focus is a separate variant.
    assert_eq!(Palette::SELECTION_BG, Palette::SELECTION);
    assert_ne!(Palette::SELECTION_BG, Palette::selection_bg());
    // C5 — the 1px separator reuses the raised surface, not the LINE border.
    assert_eq!(Palette::DIVIDER, Palette::RAISED);
    assert_ne!(Palette::DIVIDER, Palette::LINE);
    // S3 — chip/control radii are shared variants, distinct from window/menu.
    assert_eq!(Palette::RADIUS_CHIP, theme::CHIP_RADIUS);
    assert_eq!(Palette::RADIUS_CONTROL, theme::CONTROL_RADIUS);
    assert_ne!(theme::WINDOW_RADIUS, theme::CHIP_RADIUS);
    // S2 — density roles are genuinely tighter than the default control
    // (static property: the values are compile-time constants).
    const { assert!(theme::DENSITY_DENSE_BUTTON.y < theme::DENSITY_COMPACT_BUTTON.y) };
    // Severity family keeps its three meanings distinct (C2 semantics).
    assert_ne!(Palette::STATE_INFO, Palette::STATE_WARNING);
    assert_ne!(Palette::STATE_WARNING, Palette::STATE_ERROR);
}
