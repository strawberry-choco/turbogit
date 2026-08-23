//! Central dark-only design token set (ADR-0003, issue #3).
//!
//! Every color used by the app derives from the single [`Palette`] token
//! struct below, mirroring the mockups' `colors_and_type.css`. There is
//! exactly one palette — widgets never branch on a theme mode because no
//! other mode exists. One call to [`configure_style`] maps the tokens into
//! egui `Visuals`; [`install_fonts`] applies the embedded type stack.

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2, Visuals};

/// The single central token set (spec §2, Darcula-derived dark).
pub struct Palette;

impl Palette {
    // Core surfaces & lines.
    /// App background, panel fill (`--tg-bg`).
    pub const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
    /// Topbar, headers, dialogs (`--tg-surface`).
    pub const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
    /// Hover fills, secondary buttons (`--tg-surface-2`).
    pub const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
    /// Inputs, popovers (`--tg-surface-3`).
    pub const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
    /// Primary borders (`--tg-line`).
    pub const LINE: Color32 = Color32::from_rgb(0x4e, 0x51, 0x57);
    /// Subtle row separators (`--tg-line-subtle`).
    pub const LINE_SUBTLE: Color32 = Color32::from_rgb(0x36, 0x38, 0x3c);

    // Ink (text).
    /// Primary text (`--tg-ink`).
    pub const INK: Color32 = Color32::from_rgb(0xbc, 0xbe, 0xc4);
    /// Secondary text (`--tg-ink-2`).
    pub const INK_2: Color32 = Color32::from_rgb(0xa0, 0xa3, 0xab);
    /// Muted/hint text (`--tg-ink-3`).
    pub const INK_3: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);

    // Brand.
    /// Primary actions, selection, links (`--tg-brand`).
    pub const BRAND: Color32 = Color32::from_rgb(0x35, 0x74, 0xf0);
    /// Text on brand-colored fills (`--tg-brand-ink`).
    pub const BRAND_INK: Color32 = Color32::WHITE;

    // Status colors.
    /// Success (`--tg-state-success`).
    pub const STATE_SUCCESS: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);
    /// Warning (`--tg-state-warning`).
    pub const STATE_WARNING: Color32 = Color32::from_rgb(0xf9, 0xa8, 0x25);
    /// Error (`--tg-state-error`).
    pub const STATE_ERROR: Color32 = Color32::from_rgb(0xef, 0x53, 0x50);
    /// Info (`--tg-state-info`).
    pub const STATE_INFO: Color32 = Color32::from_rgb(0x42, 0xa5, 0xf5);

    // Diff colors.
    /// Added-line background (`--tg-diff-add`).
    pub const DIFF_ADD_BG: Color32 = Color32::from_rgb(0x34, 0x4f, 0x3e);
    /// Added-line text (`--tg-diff-add-text`).
    pub const DIFF_ADD_TEXT: Color32 = Color32::from_rgb(0x85, 0xe8, 0x9d);
    /// Deleted-line background (`--tg-diff-del`).
    pub const DIFF_DEL_BG: Color32 = Color32::from_rgb(0x5a, 0x3a, 0x3a);
    /// Deleted-line text (`--tg-diff-del-text`).
    pub const DIFF_DEL_TEXT: Color32 = Color32::from_rgb(0xff, 0x9a, 0x9a);

    /// Selected-row fill: BRAND at ~25% premultiplied alpha over BG.
    pub fn selection_bg() -> Color32 {
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    }
}

/// Accent (selection / primary action) color — the brand token.
pub fn accent() -> Color32 {
    Palette::BRAND
}

/// Muted icon/foreground tint that reads well on the dark palette.
pub fn icon_color() -> Color32 {
    Palette::INK_2
}

fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.window_fill = Palette::SURFACE;
    v.panel_fill = Palette::BG;
    v.extreme_bg_color = Palette::BG;
    v.override_text_color = Some(Palette::INK);
    v.faint_bg_color = Palette::SURFACE_2;
    v.code_bg_color = Palette::SURFACE_3;
    v.hyperlink_color = Palette::BRAND;
    v.warn_fg_color = Palette::STATE_WARNING;
    v.error_fg_color = Palette::STATE_ERROR;
    v.selection.bg_fill = Palette::selection_bg();
    v.selection.stroke = Stroke::new(1.0, Palette::BRAND);
    v.widgets.noninteractive.bg_fill = Palette::SURFACE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Palette::INK_2);
    v.widgets.inactive.bg_fill = Palette::SURFACE_2;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Palette::INK_2);
    v.widgets.hovered.bg_fill = Palette::SURFACE_2;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Palette::INK);
    v.widgets.active.bg_fill = Palette::SURFACE_3;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Palette::INK);
    v.widgets.open.bg_fill = Palette::SURFACE_3;
    v.widgets.open.fg_stroke = Stroke::new(1.0, Palette::INK);
    // Active window headers blend with the SURFACE window fill (issue #22).
    v.widgets.open.weak_bg_fill = Palette::SURFACE;
    v.window_corner_radius = CornerRadius::same(8); // radius-lg
    v.menu_corner_radius = CornerRadius::same(6); // radius-md
                                                  // Popup chrome (issue #22, spec §10): every floating surface (dialogs,
                                                  // popups, palette, toast) gets the LINE border stroke over its SURFACE
                                                  // fill; radius + fill are already mapped above.
    v.window_stroke = Stroke::new(1.0, Palette::LINE);
    v
}

/// Apply the dark-only design `Visuals` plus a shared spacing / typography
/// scale. Idempotent; called from the app loop.
pub fn configure_style(ctx: &Context) {
    ctx.all_styles_mut(|style| {
        style.visuals = dark_visuals();

        // Spacing scale (Epic A4).
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(10);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.indent = 14.0;

        // Typography scale (Epic A4): slightly larger body, mono for code.
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );

        style.animation_time = 0.12;
    });
}

// --- Cycle 3 implementation: embedded fonts (ADR-0002) ---

use egui::epaint::text::{FontData, FontDefinitions, FontTweak};

/// Embedded JetBrains Mono Regular (OFL-licensed; see `assets/fonts/OFL.txt`).
pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
/// Embedded JetBrains Mono Bold.
pub const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");

/// Registry keys used in [`font_definitions`].
const JBM_REGULAR_KEY: &str = "jetbrains-mono-regular";
const JBM_BOLD_KEY: &str = "jetbrains-mono-bold";

/// Load a system font file, if present on this machine. System faces are
/// fallback-only chain entries (ADR-0002): when the file cannot be found the
/// face is omitted from the chains entirely — epaint panics on family members
/// without font data, so absent names must never be listed.
fn system_font_data(file_names: &[&str]) -> Option<std::borrow::Cow<'static, [u8]>> {
    let mut roots = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        roots.push(std::path::PathBuf::from(windir).join("Fonts"));
    }
    roots.push(std::path::PathBuf::from("C:\\Windows\\Fonts"));

    for root in &roots {
        for name in file_names {
            let path = root.join(name);
            if let Ok(data) = std::fs::read(&path) {
                return Some(std::borrow::Cow::Owned(data));
            }
        }
    }
    None
}

/// Build the design font stack: embedded JetBrains Mono Regular+Bold as the
/// primary faces, followed by egui's built-in glyph fallbacks and — when
/// present on this machine — the Windows system faces (`Segoe UI`,
/// `Consolas`). System faces are consulted only for glyphs missing from JBM
/// (e.g. CJK) and degrade gracefully by omission when absent.
///
/// Per ADR-0002 the primary faces are always embedded binary includes so text
/// metrics are deterministic across machines; system lookup would vary layout.
pub fn font_definitions() -> FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert(
        JBM_REGULAR_KEY.into(),
        std::sync::Arc::new(FontData {
            font: std::borrow::Cow::Borrowed(JETBRAINS_MONO_REGULAR),
            index: 0,
            tweak: FontTweak::default(),
        }),
    );
    defs.font_data.insert(
        JBM_BOLD_KEY.into(),
        std::sync::Arc::new(FontData {
            font: std::borrow::Cow::Borrowed(JETBRAINS_MONO_BOLD),
            index: 0,
            tweak: FontTweak::default(),
        }),
    );

    // Optional system fallbacks: registered only when actually loadable.
    let segoe_ui_available = system_font_data(&["segoeui.ttf"]);
    if let Some(data) = segoe_ui_available.clone() {
        defs.font_data.insert(
            "Segoe UI".into(),
            std::sync::Arc::new(FontData {
                font: data,
                index: 0,
                tweak: FontTweak::default(),
            }),
        );
    }
    let consolas_available = system_font_data(&["consola.ttf"]);
    if let Some(data) = consolas_available.clone() {
        defs.font_data.insert(
            "Consolas".into(),
            std::sync::Arc::new(FontData {
                font: data,
                index: 0,
                tweak: FontTweak::default(),
            }),
        );
    }

    // Proportional = mono-everything look of the mockups; keep egui's
    // built-in glyph fallbacks behind JBM, then the optional system face.
    let mut proportional = vec![JBM_REGULAR_KEY.to_owned()];
    proportional.extend(
        defs.families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    if segoe_ui_available.is_some() {
        proportional.push("Segoe UI".to_owned());
    }
    defs.families
        .insert(egui::FontFamily::Proportional, proportional);

    // Monospace = JBM with optional Consolas fallback behind the defaults.
    let mut monospace = vec![JBM_REGULAR_KEY.to_owned()];
    monospace.extend(
        defs.families
            .get(&egui::FontFamily::Monospace)
            .cloned()
            .unwrap_or_default(),
    );
    if consolas_available.is_some() {
        monospace.push("Consolas".to_owned());
    }
    defs.families.insert(egui::FontFamily::Monospace, monospace);

    // Named bold family for real bold rendering via FontId.
    defs.families.insert(
        egui::FontFamily::Name("jetbrains-mono-bold".into()),
        vec![JBM_BOLD_KEY.into()],
    );

    defs
}

/// Install the embedded design font stack into the context (ADR-0002).
/// Call once at startup, before the first frame; takes effect at the
/// next pass begin.
pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
}
