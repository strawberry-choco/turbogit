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
    /// Sidebar surface (`#1B1C1E`) — dedicated to the Local Changes redesign;
    /// darker than every window/panel fill, kept out of the general surface
    /// ladder so a sidebar can never be mistaken for content chrome.
    pub const SIDEBAR: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1e);

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

    // Risk scale (issue #01): three severity tiers aliased onto the status
    // family so a risk surface never invents a hue.
    /// Low-risk surface color (issue #01).
    pub const RISK_LOW: Color32 = Self::STATE_SUCCESS;
    /// Medium-risk surface color (issue #01).
    pub const RISK_MEDIUM: Color32 = Self::STATE_WARNING;
    /// High-risk surface color (issue #01).
    pub const RISK_HIGH: Color32 = Self::STATE_ERROR;

    // Status semantics (issue #01): clean / dirty / diverged / stale mapped
    // onto the four state tokens — every status chip paints a real color,
    // never a fallback.
    /// Working tree matches HEAD (no local edits) — issue #01.
    pub const STATUS_CLEAN: Color32 = Self::STATE_SUCCESS;
    /// Working tree has uncommitted edits — issue #01.
    pub const STATUS_DIRTY: Color32 = Self::STATE_WARNING;
    /// Local branch has diverged from upstream — issue #01.
    pub const STATUS_DIVERGED: Color32 = Self::STATE_ERROR;
    /// Cached state is older than the source it mirrors — issue #01.
    pub const STATUS_STALE: Color32 = Self::STATE_INFO;

    // --- Branches screen token set (design doc §13) ------------------------
    // The Branches screen adds its own surface/meaning/text vocabulary beside
    // the core palette. Values are the design reference's own; where a §13
    // surface coincides with an existing token (RAISED == SURFACE, DIVIDER ==
    // LINE) the existing token is aliased so the whole app keeps one source.

    /// Window / bars (`#1A1B1E`): title bar, activity strip, status bar.
    pub const WINDOW_BG: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x1e);
    /// Panel (`#1E2023`): sidebar, metadata panel, detail panel, input wells.
    pub const PANEL_BG: Color32 = Color32::from_rgb(0x1e, 0x20, 0x23);
    /// Content (`#232529`): branch list, breadcrumb strip, tab strip.
    pub const CONTENT_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
    /// Raised control (`#2B2D30`): secondary buttons, badges, chips, active tab.
    pub const RAISED: Color32 = Self::SURFACE;
    /// Selection (`#2E4369`): the active row/pill.
    pub const SELECTION: Color32 = Color32::from_rgb(0x2e, 0x43, 0x69);
    /// Divider (`#2B2D30`): 1px separators.
    pub const DIVIDER: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);

    /// Accent / primary action (`#3574F0`): New Branch, Checkout, branch chips.
    pub const ACCENT: Color32 = Self::BRAND;
    /// Ahead (`#5FA86C`): ahead counts, current-branch icon, "in sync".
    pub const AHEAD: Color32 = Color32::from_rgb(0x5f, 0xa8, 0x6c);
    /// Behind (`#DCA34E`): behind counts, dirty markers.
    pub const BEHIND: Color32 = Color32::from_rgb(0xdc, 0xa3, 0x4e);
    /// Danger (`#DB5C5C`): Delete only.
    pub const DANGER: Color32 = Color32::from_rgb(0xdb, 0x5c, 0x5c);
    /// Link / hash (`#74A3E8`): commit hash chips.
    pub const LINK: Color32 = Color32::from_rgb(0x74, 0xa3, 0xe8);

    /// Text ramp — primary (`#DFE1E5`): branch names, body text.
    pub const T_PRIMARY: Color32 = Color32::from_rgb(0xdf, 0xe1, 0xe5);
    /// Text ramp — secondary (`#9DA0A8`): section labels, secondary actions.
    pub const T_SECONDARY: Color32 = Color32::from_rgb(0x9d, 0xa0, 0xa8);
    /// Text ramp — muted (`#6F737B`): counts, timestamps, placeholders.
    pub const T_MUTED: Color32 = Color32::from_rgb(0x6f, 0x73, 0x7b);

    /// Corner radius for chips and badges (design doc §13: 3).
    pub const RADIUS_CHIP: u8 = 3;
    /// Corner radius for buttons, inputs and panels (design doc §13: 4).
    pub const RADIUS_CONTROL: u8 = 4;

    // Reserved counter orange (redesign issue 01): `#E0883C` is reserved for
    // dirt/unpulled count badges only. Never a general accent, action, or
    // warning — keep it out of any non-counter call site.
    /// Dirty/unpulled counter orange.
    pub const COUNTER: Color32 = Color32::from_rgb(0xe0, 0x88, 0x3c);

    // Local Changes status letters (redesign issue 01): M / A / U colour the
    // status letter and the filename in a file row (mockup values).
    /// Modified file letter (`M`) — light blue.
    pub const STATUS_MODIFIED: Color32 = Color32::from_rgb(0xa8, 0xc0, 0xe8);
    /// Added file letter (`A`) — the diff-added green.
    pub const STATUS_ADDED: Color32 = Color32::from_rgb(0x57, 0x96, 0x5c);
    /// Unversioned file letter (`U`) — olive.
    pub const STATUS_UNVERSIONED: Color32 = Color32::from_rgb(0xb5, 0xb3, 0x7e);

    // Diff colors.
    /// Added-line background (`--tg-diff-add`).
    pub const DIFF_ADD_BG: Color32 = Color32::from_rgb(0x34, 0x4f, 0x3e);
    /// Added-line text (`--tg-diff-add-text`).
    pub const DIFF_ADD_TEXT: Color32 = Color32::from_rgb(0x85, 0xe8, 0x9d);
    /// Deleted-line background (`--tg-diff-del`).
    pub const DIFF_DEL_BG: Color32 = Color32::from_rgb(0x5a, 0x3a, 0x3a);
    /// Deleted-line text (`--tg-diff-del-text`).
    pub const DIFF_DEL_TEXT: Color32 = Color32::from_rgb(0xff, 0x9a, 0x9a);

    // Redesign diff tokens (issue 01): accent (line text/markers) on a tinted
    // block (line background) for the new diff preview — distinct from the
    // legacy DIFF_*_BG/TEXT pair above, which the old diff view keeps using.
    /// Added-line accent (`#57965C`) — shares the added-file green.
    pub const DIFF_ADD_ACCENT: Color32 = Color32::from_rgb(0x57, 0x96, 0x5c);
    /// Added-line block (`#2E4334`).
    pub const DIFF_ADD_BLOCK: Color32 = Color32::from_rgb(0x2e, 0x43, 0x34);
    /// Removed-line accent (`#F75464`).
    pub const DIFF_DEL_ACCENT: Color32 = Color32::from_rgb(0xf7, 0x54, 0x64);
    /// Removed-line block (`#433034`).
    pub const DIFF_DEL_BLOCK: Color32 = Color32::from_rgb(0x43, 0x30, 0x34);

    /// Selected-row fill: BRAND at ~25% premultiplied alpha over BG.
    pub fn selection_bg() -> Color32 {
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    }

    /// Solid row-selection background (`#2E436E`) for the Local Changes
    /// redesign. Distinct from the translucent [`Self::selection_bg`] the
    /// shell rows paint today; later tickets opt in by switching call sites.
    pub const SELECTION_BG: Color32 = Color32::from_rgb(0x2e, 0x43, 0x6e);
}

// --- Spacing scale (Local Changes redesign, design doc §8) ---
// Dimension tokens, kept beside the color palette as part of the single
// central token set. Rows keep one consistent height per kind; gaps and
// padding sit on the 4 px grid.
/// File-row height in the changes tree (24 px). This is the single-line
/// height: a row grows one text line per extra line of text, because a
/// filename too long for its column wraps rather than clipping.
pub const FILE_ROW_HEIGHT: f32 = 24.0;
/// Group-row height in the changes tree (26 px).
pub const GROUP_ROW_HEIGHT: f32 = 26.0;
/// Grid-gap base unit; all gaps are multiples of 4 px.
pub const GRID_GAP: f32 = 4.0;
/// Panel padding (12 px; the spec allows 12–14 px).
pub const PANEL_PADDING: f32 = 12.0;

/// Accent (selection / primary action) color — the brand token.
pub fn accent() -> Color32 {
    Palette::BRAND
}

/// Muted icon/foreground tint that reads well on the dark palette.
pub fn icon_color() -> Color32 {
    Palette::INK_2
}

// --- Branches-screen type ramp (design doc §13) -------------------------
// Five sizes are enough for the whole screen: 9 section labels, 10 chips,
// 11 controls/metadata, 12 branch names/body, 13 the detail title.
/// Section labels — 9px, uppercase, letter-spaced (rendered via `section_label`).
pub const TYPE_SECTION: f32 = 9.0;
/// Chips and badges — 10px.
pub const TYPE_CHIP: f32 = 10.0;
/// Controls, key labels, metadata — 11px.
pub const TYPE_CONTROL: f32 = 11.0;
/// Branch names and body text — 12px.
pub const TYPE_BODY: f32 = 12.0;
/// Detail-panel title — 13px.
pub const TYPE_DETAIL_TITLE: f32 = 13.0;

/// Data font face — JetBrains Mono (branch names, hashes, paths, upstreams,
/// counts, timestamps).
pub fn data_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Chrome font face — Inter when it is registered; today the chassis embeds
/// only JetBrains Mono (ADR-0002), so chrome falls back to the proportional
/// family. Keeping the seam means a future Inter registration upgrades every
/// chrome call site at once (buttons, tabs, section headers, labels).
pub fn chrome_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
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
