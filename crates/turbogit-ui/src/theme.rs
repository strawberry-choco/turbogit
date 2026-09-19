//! Central dark-only design token set (ADR-0003, issue #3).
//!
//! [`Palette`] below is the single central token set for the *shared*
//! presentation roles — surfaces, ink hierarchy, selection, severity,
//! status/diff families, spacing, density and chip shapes — mirroring the
//! mockups' `colors_and_type.css`. There is exactly one palette and widgets
//! never branch on a theme mode because no other mode exists.
//!
//! Ownership boundary (G1): not every presentation value in the app is
//! centralized here — screen- or component-specific vocabularies kept local
//! to their consumers (e.g. the log graph palette and the cascade accent)
//! remain where they are used, and Review-only C6/T3 families are out of
//! scope. The shared roles below ARE the authoritative contract: changing one
//! of these tokens reaches every consumer that uses the role. Enforcement
//! lives in the existing design-token, widget-library and branch-component
//! suites (`tests/design_tokens.rs`, `tests/widget_library.rs`,
//! `tests/branch_component_kit.rs`).
//!
//! One call to [`configure_style`] maps the tokens into egui `Visuals`;
//! [`install_fonts`] applies the embedded type stack.

use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Ui, Vec2, Visuals,
};

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
    /// Primary text (`--tg-ink`). Equal-value alias of the authoritative
    /// `T_PRIMARY` body ink — one primary role across shell defaults and
    /// tool-window content (C3), so a central ramp change reaches both.
    pub const INK: Color32 = Self::T_PRIMARY;
    /// Secondary text (`--tg-ink-2`).
    pub const INK_2: Color32 = Self::T_SECONDARY;
    /// Muted/hint text (`--tg-ink-3`).
    pub const INK_3: Color32 = Self::T_MUTED;

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
    // surface carries the same value as an existing token the existing token
    // is declared as the value's source so the whole app keeps one definition
    // (RAISED aliases SURFACE, and DIVIDER aliases RAISED — the 1px separator
    // uses the raised-surface tone, NOT the stronger LINE border).
    //
    // Supported surfaces and their intended compositing backgrounds (C5):
    // all fills here are fully opaque and composite directly onto the window
    // background; translucent treatments (e.g. the rgba selection fill) are
    // defined against the specific surface they sit on and documented with it.

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
    /// Group-label band (`#1F232C`): the scaffolding strip a `LOCAL` / `REMOTE`
    /// header sits on. A faint cool lift where the repo header, the hover fill
    /// and the raised surfaces are all neutral grey, and far weaker than a
    /// current row's band — so group labels never read as a row state.
    pub const SECTION_BG: Color32 = Color32::from_rgb(0x1f, 0x23, 0x2c);
    /// Divider (`#2B2D30`): 1px separators. An equal-valued semantic alias of
    /// the raised surface (RAISED/SURFACE) — separators read as hairline
    /// relief against the raised tone, not as the stronger LINE border.
    pub const DIVIDER: Color32 = Self::RAISED;

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
    /// Secondary text, including on selected rows and raised controls.
    pub const T_SECONDARY: Color32 = Color32::from_rgb(0xb0, 0xb3, 0xbb);
    /// Muted text: AA contrast (≥4.5:1) on every surface muted ink paints on,
    /// including the SELECTION fill.
    pub const T_MUTED: Color32 = Color32::from_rgb(0xae, 0xb2, 0xba);
    /// Readable accent ink. Action fills continue to use ACCENT/BRAND.
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x8b, 0xb5, 0xf5);

    /// Corner radius for chips and badges (design doc §13: 3) — shared
    /// `CHIP_RADIUS` variant (S3).
    pub const RADIUS_CHIP: u8 = CHIP_RADIUS;
    /// Corner radius for buttons, inputs and panels (design doc §13: 4) —
    /// shared `CONTROL_RADIUS` variant (S3).
    pub const RADIUS_CONTROL: u8 = CONTROL_RADIUS;

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

    /// Solid row-selection background for the Local Changes
    /// redesign. Equal-value alias of the canonical [`Self::SELECTION`]:
    /// commit inclusion and navigation selection share one opaque selection
    /// fill (C4) — previously a near-duplicate #2E436E; the translucent
    /// [`Self::selection_bg`] focus treatment remains a separate variant.
    pub const SELECTION_BG: Color32 = Self::SELECTION;
}

/// Shared repository-state vocabulary for dots, badges and summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RepoState {
    Clean,
    Dirty,
    Conflict,
    Diverged,
    Unpushed,
    Unpulled,
}

impl RepoState {
    /// The state in words, beside [`RepoState::color`] so a dot and the summary
    /// next to it can never disagree. The wording is the sidebar's smart-group
    /// vocabulary where the states overlap; `in sync` is new, and is what a
    /// repository with nothing to report says.
    pub fn words(self) -> &'static str {
        match self {
            Self::Clean => "in sync",
            Self::Dirty => "dirty worktree",
            Self::Conflict => "has conflicts",
            Self::Diverged => "diverged",
            Self::Unpushed => "unpushed commits",
            Self::Unpulled => "unpulled commits",
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Self::Clean | Self::Unpushed => Palette::AHEAD,
            Self::Dirty | Self::Unpulled => Palette::COUNTER,
            Self::Conflict | Self::Diverged => Palette::STATUS_DIVERGED,
        }
    }

    /// Conflicts and local edits take precedence over upstream drift.
    pub fn from_root(root: &turbogit_domain::model::Root, ahead: usize, behind: usize) -> Self {
        if !root.status.conflicted.is_empty() {
            Self::Conflict
        } else if root.status.modified() + root.status.unversioned() > 0 {
            Self::Dirty
        } else if ahead > 0 && behind > 0 {
            Self::Diverged
        } else if behind > 0 {
            Self::Unpulled
        } else if ahead > 0 {
            Self::Unpushed
        } else {
            Self::Clean
        }
    }
}

// --- Spacing scale (Local Changes redesign, design doc §8) ---
// Dimension tokens, kept beside the color palette as part of the single
// central token set. Rows keep one consistent height per kind; the default
// gaps/padding/margins below ARE the shared layout contract (S1) — the
// 4 px grid describes row heights and panel padding, not every gap (the
// default item spacing and control padding are named roles of their own).
/// File-row height in the changes tree (24 px). This is the single-line
/// height: a row grows one text line per extra line of text, because a
/// filename too long for its column wraps rather than clipping.
pub const FILE_ROW_HEIGHT: f32 = 24.0;
/// Group-row height in the changes tree (26 px).
pub const GROUP_ROW_HEIGHT: f32 = 26.0;
/// Grid-gap base unit for row/group heights and panel padding.
pub const GRID_GAP: f32 = 4.0;
/// Panel padding (12 px; the spec allows 12–14 px).
pub const PANEL_PADDING: f32 = 12.0;

// Shared layout roles (S1): the one source for the shell defaults.
/// Default item gap between widgets (8×6) — installed by [`configure_style`].
pub const ITEM_SPACING: Vec2 = Vec2::new(8.0, 6.0);
/// Default window/panel margin (10 px) — installed by [`configure_style`].
pub const WINDOW_MARGIN: i8 = 10;
/// Default control padding (10×5) — installed by [`configure_style`].
pub const BUTTON_PADDING: Vec2 = Vec2::new(10.0, 5.0);
/// Default list/tree indent (14 px) — installed by [`configure_style`].
pub const INDENT: f32 = 14.0;

// Density variants (S2): named exceptions to the defaults above, so compact
// and dense regions stop carrying regional literals.
/// Compact control padding (8×4) for dense shell/toolbar regions.
pub const DENSITY_COMPACT_BUTTON: Vec2 = Vec2::new(8.0, 4.0);
/// Dense control padding (6×2) for the status bar.
pub const DENSITY_DENSE_BUTTON: Vec2 = Vec2::new(6.0, 2.0);

// Shape variants (S3): chip/control radii are explicit shared roles; the
// pill variant is the full-height wrap the welcome chips and badges use.
/// Compact chip corner radius (3 px).
pub const CHIP_RADIUS: u8 = 3;
/// Control corner radius (4 px).
pub const CONTROL_RADIUS: u8 = 4;
/// Pill corner radius (9 px) — full-height pill chips (welcome, badges).
pub const PILL_RADIUS: u8 = 9;
/// Window corner radius (8 px).
pub const WINDOW_RADIUS: u8 = 8;
/// Menu corner radius (6 px).
pub const MENU_RADIUS: u8 = 6;
/// Card corner radius (8 px) — the bordered content regions of the Local
/// Changes redesign, one step above the 4 px control radius so a card reads
/// as a container rather than as a large control.
pub const CARD_RADIUS: u8 = 8;

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

// Display roles (T2): distinct sizes for prominent display-only text. Named
// so central type changes reach the consumers, without reducing them to body.
/// Welcome wordmark — 42px (welcome.rs brand header).
pub const TYPE_WORDMARK: f32 = 42.0;
/// Multi-repo statistics — 22px (multi_selection.rs stats strip).
pub const TYPE_STATISTIC: f32 = 22.0;

/// Data font face — JetBrains Mono (branch names, hashes, paths, upstreams,
/// counts, timestamps).
pub fn data_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Chrome font face — the embedded UI sans, so interface labels (repo names,
/// section labels, buttons, counts) read in a proportional face while data
/// (branch and remote names, hashes, upstreams) stays monospaced. Which face
/// sits here is decided once, in [`font_definitions`]; a different chrome
/// family upgrades every call site at once.
///
/// Bold chrome has no bold member in this family, so emphasised labels keep
/// rendering through the embedded mono bold (`ui::widgets`'s bold-font path)
/// rather than a synthesised weight of the sans.
pub fn chrome_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Horizontal indent per tree depth — the width of two whitespace characters
/// in the data face. Shared by the branch tree and the sidebar project tree so
/// both hang at the same rate, off the type ramp rather than a fixed pixel.
pub fn two_space_indent(ui: &Ui) -> f32 {
    ui.painter()
        .layout_no_wrap("  ".to_owned(), data_font(TYPE_BODY), Color32::WHITE)
        .size()
        .x
}

/// How many [`two_space_indent`] steps a tree level hangs its children on. Two,
/// so a directory subgroup and the branches under it are not read as siblings.
pub const INDENT_STEPS_PER_LEVEL: f32 = 2.0;

/// The horizontal indent of one tree level — twice the two-space width, off the
/// data face.
pub fn indent_step(ui: &Ui) -> f32 {
    two_space_indent(ui) * INDENT_STEPS_PER_LEVEL
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
    v.hyperlink_color = Palette::ACCENT_TEXT;
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
    v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS); // radius-lg
    v.menu_corner_radius = CornerRadius::same(MENU_RADIUS); // radius-md
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

        // Shared layout roles (S1): the defaults below ARE the contract.
        style.spacing.item_spacing = ITEM_SPACING;
        style.spacing.window_margin = egui::Margin::same(WINDOW_MARGIN);
        style.spacing.button_padding = BUTTON_PADDING;
        style.spacing.indent = INDENT;

        // Shell defaults consume the same §13 ramp as the Branches view.
        style
            .text_styles
            .insert(TextStyle::Body, chrome_font(TYPE_BODY));
        style
            .text_styles
            .insert(TextStyle::Button, chrome_font(TYPE_CONTROL));
        style
            .text_styles
            .insert(TextStyle::Heading, chrome_font(TYPE_DETAIL_TITLE));
        style
            .text_styles
            .insert(TextStyle::Monospace, data_font(TYPE_BODY));
        style
            .text_styles
            .insert(TextStyle::Small, chrome_font(TYPE_CONTROL));

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
/// egui's built-in proportional face, already carried in
/// `FontDefinitions::default()`'s font data. Leads the chrome chain.
const UI_SANS_KEY: &str = "Ubuntu-Light";

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

/// Build the design font stack: the Monospace (data) family leads with embedded
/// JetBrains Mono Regular+Bold, the Proportional (chrome) family leads with
/// egui's embedded UI sans, and both keep egui's built-in glyph fallbacks plus —
/// when present on this machine — the Windows system faces (`Segoe UI`,
/// `Consolas`). System faces are consulted only for glyphs missing from the
/// leading faces and degrade gracefully by omission when absent.
///
/// Per ADR-0002 every leading face is embedded binary so text metrics are
/// deterministic across machines; system lookup would vary layout.
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

    // Proportional = the embedded UI sans, so chrome labels stop painting in
    // the data face. Ubuntu-Light ships inside egui (`epaint_default_fonts`),
    // which keeps ADR-0002's rule intact: an embedded binary leads the chain,
    // never a system lookup, so metrics are identical across machines.
    // egui's built-in glyph fallbacks stay behind it, so a chrome label
    // carrying a character the sans lacks still resolves to an embedded face.
    let mut proportional = vec![UI_SANS_KEY.to_owned()];
    proportional.extend(
        defs.families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|face| face != UI_SANS_KEY),
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
