use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Ui, Vec2, Visuals,
};

pub struct Palette;

impl Palette {
    pub const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
    pub const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
    pub const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
    pub const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
    pub const SURFACE_WARNING: Color32 = Color32::from_rgb(0x44, 0x3c, 0x2f);
    pub const SURFACE_INFLIGHT_ABORT: Color32 = Color32::from_rgb(0x40, 0x33, 0x35);
    pub const LINE: Color32 = Color32::from_rgb(0x4e, 0x51, 0x57);
    pub const LINE_SUBTLE: Color32 = Color32::from_rgb(0x36, 0x38, 0x3c);

    pub const INK: Color32 = Self::T_PRIMARY;
    pub const INK_2: Color32 = Self::T_SECONDARY;
    pub const INK_3: Color32 = Self::T_MUTED;
    pub const INK_4: Color32 = Self::T_DIM;

    pub const BRAND: Color32 = Color32::from_rgb(0x35, 0x74, 0xf0);
    pub const BRAND_INK: Color32 = Color32::WHITE;

    pub const STATE_SUCCESS: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);
    pub const STATE_WARNING: Color32 = Color32::from_rgb(0xf9, 0xa8, 0x25);
    pub const STATE_ERROR: Color32 = Color32::from_rgb(0xef, 0x53, 0x50);
    pub const STATE_INFO: Color32 = Color32::from_rgb(0x42, 0xa5, 0xf5);

    pub const STATUS_DIVERGED: Color32 = Self::STATE_ERROR;

    pub const CONTENT_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
    pub const RAISED: Color32 = Self::SURFACE;
    pub const RAISED_ON_CARD: Color32 = Self::SURFACE_2;
    pub const SELECTION: Color32 = Color32::from_rgb(0x2e, 0x43, 0x69);
    pub const ROW_SELECTED: Color32 = Color32::from_rgb(0x24, 0x34, 0x56);
    pub const SECTION_BG: Color32 = Color32::from_rgb(0x1f, 0x23, 0x2c);

    pub const RULE_CONTENT: Color32 = Self::RAISED;
    pub const RULE_FOOTER: Color32 = Self::LINE;
    pub const RULE_STRUCTURAL: Color32 = Self::LINE_SUBTLE;

    pub const ACCENT: Color32 = Self::BRAND;
    pub const AHEAD: Color32 = Color32::from_rgb(0x5f, 0xa8, 0x6c);
    pub const DANGER: Color32 = Color32::from_rgb(0xdb, 0x5c, 0x5c);
    pub const LINK: Color32 = Color32::from_rgb(0x74, 0xa3, 0xe8);

    pub const T_PRIMARY: Color32 = Color32::from_rgb(0xdf, 0xe1, 0xe5);
    pub const T_SECONDARY: Color32 = Color32::from_rgb(0xb0, 0xb3, 0xbb);
    pub const T_MUTED: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x96);
    pub const T_DIM: Color32 = Color32::from_rgb(0x6e, 0x72, 0x7a);
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x8b, 0xb5, 0xf5);

    pub const COUNTER: Color32 = Color32::from_rgb(0xe0, 0x88, 0x3c);

    pub const STATUS_MODIFIED: Color32 = Color32::from_rgb(0xa8, 0xc0, 0xe8);
    pub const STATUS_ADDED: Color32 = Color32::from_rgb(0x6f, 0xae, 0x75);
    pub const STATUS_UNVERSIONED: Color32 = Color32::from_rgb(0xb5, 0xb3, 0x7e);
    pub const STATUS_DELETED: Color32 = Self::T_MUTED;

    pub const DIFF_ADD_BG: Color32 = Color32::from_rgb(0x34, 0x4f, 0x3e);
    pub const DIFF_ADD_TEXT: Color32 = Color32::from_rgb(0x85, 0xe8, 0x9d);
    pub const DIFF_DEL_BG: Color32 = Color32::from_rgb(0x5a, 0x3a, 0x3a);
    pub const DIFF_DEL_TEXT: Color32 = Color32::from_rgb(0xff, 0x9a, 0x9a);

    pub const DIFF_ADD_ACCENT: Color32 = Color32::from_rgb(0x6f, 0xae, 0x75);
    pub const DIFF_DEL_ACCENT: Color32 = Color32::from_rgb(0xf7, 0x54, 0x64);

    pub const DIFF_ADD_UNSTAGED_BG: Color32 = Color32::from_rgb(0x2a, 0x36, 0x31);
    pub const DIFF_ADD_UNSTAGED_TEXT: Color32 = Self::DIFF_ADD_ACCENT;
    pub const DIFF_DEL_UNSTAGED_BG: Color32 = Color32::from_rgb(0x39, 0x2d, 0x30);
    pub const DIFF_DEL_UNSTAGED_TEXT: Color32 = Color32::from_rgb(0xd9, 0x83, 0x83);

    pub fn selection_bg() -> Color32 {
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    }

    pub const SIDEBAR: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1e);
}

pub const SECTION_TINT: f32 = 0.12;
pub const MARKER_TINT: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RepoState {
    Clean,
    Dirty,
    Conflict,
    Diverged,
    Unpushed,
    Unpulled,
    Uninitialized,
}

impl RepoState {
    pub fn words(self) -> &'static str {
        match self {
            Self::Clean => "",
            Self::Dirty => "dirty",
            Self::Conflict => "has conflicts",
            Self::Diverged => "diverged",
            Self::Unpushed => "unpushed commits",
            Self::Unpulled => "unpulled commits",
            Self::Uninitialized => "not initialized",
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Self::Clean | Self::Unpushed => Palette::AHEAD,
            Self::Dirty | Self::Unpulled => Palette::COUNTER,
            Self::Conflict | Self::Diverged => Palette::STATUS_DIVERGED,
            Self::Uninitialized => Palette::INK_3,
        }
    }

    pub fn from_root(root: &turbogit_domain::model::Root, ahead: usize, behind: usize) -> Self {
        if !root.status.conflicted.is_empty() {
            Self::Conflict
        } else if Self::is_dirty(&root.status) {
            Self::Dirty
        } else if Self::is_diverged(ahead, behind) {
            Self::Diverged
        } else if behind > 0 {
            Self::Unpulled
        } else if ahead > 0 {
            Self::Unpushed
        } else {
            Self::Clean
        }
    }

    pub fn is_diverged(ahead: usize, behind: usize) -> bool {
        ahead > 0 && behind > 0
    }

    pub fn has_drift(ahead: usize, behind: usize) -> bool {
        ahead > 0 || behind > 0
    }

    pub fn is_dirty(status: &turbogit_domain::model::RootStatus) -> bool {
        status.modified() + status.unversioned() > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DivergenceMeaning {
    Both,
    AnyDrift,
}

impl DivergenceMeaning {
    pub fn is_diverged(self, ahead: usize, behind: usize) -> bool {
        match self {
            Self::Both => RepoState::is_diverged(ahead, behind),
            Self::AnyDrift => RepoState::has_drift(ahead, behind),
        }
    }
}

pub const FILE_ROW_HEIGHT: f32 = 24.0;
pub const GROUP_ROW_HEIGHT: f32 = 26.0;
pub const PANEL_PADDING: f32 = 12.0;

pub const ITEM_SPACING: Vec2 = Vec2::new(8.0, 6.0);
pub const WINDOW_MARGIN: i8 = 10;
pub const BUTTON_PADDING: Vec2 = Vec2::new(10.0, 5.0);
pub const INDENT: f32 = 14.0;
pub const CELL_GAP: f32 = 8.0;

pub const DENSITY_COMPACT_BUTTON: Vec2 = Vec2::new(8.0, 4.0);
pub const DENSITY_DENSE_BUTTON: Vec2 = Vec2::new(6.0, 2.0);
pub const DENSITY_COMMAND_BUTTON: Vec2 = Vec2::new(8.0, 1.0);
pub const DENSITY_COMMAND_ROW_HEIGHT: f32 = 14.0;
pub const DENSITY_LIST_GAP: f32 = 2.0;
pub const STATUS_INFLIGHT_ABORT_GAP: f32 = 12.0;

pub const CHIP_RADIUS: u8 = 3;
pub const CONTROL_RADIUS: u8 = 4;
pub const WINDOW_RADIUS: u8 = 8;
pub const MENU_RADIUS: u8 = 6;
pub const CARD_RADIUS: u8 = 8;
pub const PILL_RADIUS: u8 = 9;
pub const CASCADE_ACCENT: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);
pub const MARK_RADIUS: u8 = 2;
pub const RAIL_WIDTH: f32 = 2.0;

pub const CARET_WIDTH: f32 = 2.0;

pub fn accent() -> Color32 {
    Palette::BRAND
}

pub fn icon_color() -> Color32 {
    Palette::INK_2
}

pub const TYPE_SECTION: f32 = 9.0;
pub const TYPE_CHIP: f32 = 10.0;
pub const TYPE_CONTROL: f32 = 11.0;
pub const TYPE_BODY: f32 = 12.0;
pub const TYPE_DETAIL_TITLE: f32 = 13.0;

pub const TYPE_GROUP_NAME: f32 = 12.5;
pub const TYPE_PANE_TITLE: f32 = 16.0;

pub const TYPE_WORDMARK: f32 = 42.0;
pub const TYPE_TAGLINE: f32 = 14.0;
pub const TYPE_STATISTIC: f32 = 22.0;

pub fn data_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn chrome_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn two_space_indent(ui: &Ui) -> f32 {
    ui.painter()
        .layout_no_wrap("  ".to_owned(), data_font(TYPE_BODY), Color32::WHITE)
        .size()
        .x
}

pub const INDENT_STEPS_PER_LEVEL: f32 = 2.0;

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
    v.widgets.open.weak_bg_fill = Palette::SURFACE;
    v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS);
    v.menu_corner_radius = CornerRadius::same(MENU_RADIUS);
    v.window_stroke = Stroke::new(1.0, Palette::LINE);
    v
}

pub fn configure_style(ctx: &Context) {
    ctx.all_styles_mut(|style| {
        style.visuals = dark_visuals();

        style.spacing.item_spacing = ITEM_SPACING;
        style.spacing.window_margin = egui::Margin::same(WINDOW_MARGIN);
        style.spacing.button_padding = BUTTON_PADDING;
        style.spacing.indent = INDENT;

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

use egui::epaint::text::{FontData, FontDefinitions, FontTweak};

/// Embedded JetBrains Mono Regular (OFL-licensed; see `assets/fonts/OFL.txt`).
pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
pub const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");
/// Embedded Noto Sans SC Regular (OFL-licensed; see
/// `assets/fonts/OFL-NotoSansSC.txt`): the CJK face every chain falls back to.
pub const NOTO_SANS_SC_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Regular.ttf");

const JBM_REGULAR_KEY: &str = "jetbrains-mono-regular";
const JBM_BOLD_KEY: &str = "jetbrains-mono-bold";
const NOTO_SANS_SC_KEY: &str = "noto-sans-sc-regular";
/// egui's built-in proportional face, already carried in
/// `FontDefinitions::default()`'s font data. Leads the chrome chain.
const UI_SANS_KEY: &str = "Ubuntu-Light";

/// Load a system font file, if present on this machine. System faces are
/// fallback-only chain entries (ADR-0002): when the file cannot be found the
/// face is omitted from the chains entirely, epaint panics on family members
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
    defs.font_data.insert(
        NOTO_SANS_SC_KEY.into(),
        std::sync::Arc::new(FontData {
            font: std::borrow::Cow::Borrowed(NOTO_SANS_SC_REGULAR),
            index: 0,
            tweak: FontTweak::default(),
        }),
    );

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
    proportional.push(NOTO_SANS_SC_KEY.to_owned());
    defs.families
        .insert(egui::FontFamily::Proportional, proportional);

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
    monospace.push(NOTO_SANS_SC_KEY.to_owned());
    defs.families.insert(egui::FontFamily::Monospace, monospace);

    // Named bold family for real bold rendering via FontId. Like the two style
    // chains it keeps egui's built-in glyph fallbacks behind the embedded cut,
    // then the CJK face: alone, this chain resolved an emoji in a bold title to
    // the bold face's `?` and a Han to nothing at all. Noto joins at its regular
    // weight, because a Chinese subject needs glyphs more than a bold Han cut.
    //
    // `NotoEmoji-Regular` also keeps a coverage probe honest on this chain.
    // epaint names the first face carrying `◻`, else the first carrying `?`, as
    // the chain's replacement face, and `has_glyph` reports false for every
    // character resolving to it. With no `◻` carrier here that face is JetBrains
    // Mono Bold itself, so the chain reads as if its own Latin were missing.
    defs.families.insert(
        egui::FontFamily::Name("jetbrains-mono-bold".into()),
        vec![
            JBM_BOLD_KEY.into(),
            "NotoEmoji-Regular".into(),
            "emoji-icon-font".into(),
            NOTO_SANS_SC_KEY.into(),
        ],
    );

    defs
}

pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
}

#[cfg(test)]
mod tests {
    use super::{DivergenceMeaning, RepoState};
    use std::path::PathBuf;
    use turbogit_domain::model::{Change, ChangeStatus, Root, RootId, RootStatus};

    fn change(name: &str, status: ChangeStatus) -> Change {
        Change {
            path: PathBuf::from(name),
            status,
            chunks: vec![],
            staged: false,
            unstaged: false,
            orig_path: None,
        }
    }

    fn status_of(changes: Vec<Change>) -> RootStatus {
        RootStatus {
            changes,
            ..Default::default()
        }
    }

    fn root(status: RootStatus) -> Root {
        Root {
            id: RootId(PathBuf::from("/w/r").into()),
            path: PathBuf::from("/w/r"),
            remotes: vec![],
            branches: vec![],
            current_branch: Some("main".to_string()),
            head: None,
            status,
        }
    }

    #[test]
    fn both_histories_moved_is_what_the_ladder_calls_diverged() {
        for (ahead, behind) in [(3, 0), (0, 2), (0, 0)] {
            assert!(
                !RepoState::is_diverged(ahead, behind),
                "one side moved at ({ahead}, {behind}): the repository is unpushed or \
                 unpulled, not forked"
            );
        }
        assert!(
            RepoState::is_diverged(2, 1),
            "both sides moved: this is the state the dot, the smart group and the bulk \
             gate all mean by diverged"
        );
    }

    #[test]
    fn either_side_moved_is_what_the_loose_meaning_of_diverged_is() {
        for (ahead, behind) in [(3, 0), (0, 2)] {
            assert!(
                RepoState::has_drift(ahead, behind),
                "one side moved at ({ahead}, {behind}): the status bar counts this and \
                 the remote row marks it"
            );
        }
        assert!(
            !RepoState::has_drift(0, 0),
            "nothing moved: there is no drift to report"
        );
    }

    #[test]
    fn drift_is_never_false_where_both_histories_moved() {
        for ahead in 0..4usize {
            for behind in 0..4usize {
                if RepoState::is_diverged(ahead, behind) {
                    assert!(
                        RepoState::has_drift(ahead, behind),
                        "({ahead}, {behind}) is diverged under the ladder's meaning and \
                         must also read as drift — the two arms differ exactly on the \
                         one-sided pairs, nowhere else"
                    );
                }
            }
        }
    }

    #[test]
    fn divergence_meaning_both_rejects_the_one_sided_pairs_and_accepts_the_two_sided_one() {
        assert!(!DivergenceMeaning::Both.is_diverged(3, 0));
        assert!(!DivergenceMeaning::Both.is_diverged(0, 2));
        assert!(DivergenceMeaning::Both.is_diverged(2, 1));
    }

    #[test]
    fn divergence_meaning_any_drift_accepts_both_one_sided_pairs() {
        assert!(DivergenceMeaning::AnyDrift.is_diverged(3, 0));
        assert!(DivergenceMeaning::AnyDrift.is_diverged(0, 2));
        assert!(DivergenceMeaning::AnyDrift.is_diverged(2, 1));
    }

    #[test]
    fn is_dirty_is_modified_or_unversioned_work() {
        assert!(
            !RepoState::is_dirty(&status_of(vec![])),
            "an empty worktree is clean"
        );
        assert!(
            RepoState::is_dirty(&status_of(vec![change("a.txt", ChangeStatus::Modified)])),
            "a modified entry is local work"
        );
        assert!(
            RepoState::is_dirty(&status_of(vec![change("a.txt", ChangeStatus::Unversioned)])),
            "an unversioned entry alone is dirty here, and clean to the cherry-across \
             service — the opposite answer, and a recorded one"
        );
    }

    #[test]
    fn is_dirty_is_false_on_a_conflicted_change_entry() {
        let conflicted_entry = status_of(vec![change("c.txt", ChangeStatus::Conflicted)]);
        assert!(
            !RepoState::is_dirty(&conflicted_entry),
            "a conflicted change *entry* is not local work to the ladder, and it is \
             blocking to the cherry-across dirty rule — the two rules disagree here \
             on purpose, and both ends are pinned"
        );
    }

    fn ladder_cases() -> Vec<(&'static str, RootStatus, usize, usize, RepoState)> {
        let mut conflicted_and_dirty = status_of(vec![change("a.txt", ChangeStatus::Modified)]);
        conflicted_and_dirty.conflicted = vec![PathBuf::from("c.txt")];
        let mut conflicted_alone = status_of(vec![]);
        conflicted_alone.conflicted = vec![PathBuf::from("c.txt")];
        vec![
            (
                "a conflict beats local work, drift and both one-sided states",
                conflicted_and_dirty,
                2,
                1,
                RepoState::Conflict,
            ),
            (
                "a conflict with no other fault is still a conflict",
                conflicted_alone,
                0,
                0,
                RepoState::Conflict,
            ),
            (
                "local work beats drift",
                status_of(vec![change("a.txt", ChangeStatus::Modified)]),
                2,
                1,
                RepoState::Dirty,
            ),
            (
                "an unversioned file alone is dirty work",
                status_of(vec![change("a.txt", ChangeStatus::Unversioned)]),
                0,
                0,
                RepoState::Dirty,
            ),
            (
                "drift beats the two one-sided states",
                status_of(vec![]),
                2,
                1,
                RepoState::Diverged,
            ),
            (
                "ahead alone is unpushed",
                status_of(vec![]),
                2,
                0,
                RepoState::Unpushed,
            ),
            (
                "behind alone is unpulled",
                status_of(vec![]),
                0,
                2,
                RepoState::Unpulled,
            ),
            (
                "nothing at all is clean",
                status_of(vec![]),
                0,
                0,
                RepoState::Clean,
            ),
        ]
    }

    #[test]
    fn from_root_resolves_one_state_per_precedence_rung() {
        for (case, status, ahead, behind, expected) in ladder_cases() {
            assert_eq!(
                RepoState::from_root(&root(status), ahead, behind),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn from_root_agrees_with_the_predicates_it_is_composed_from() {
        for (case, status, ahead, behind, _) in ladder_cases() {
            let composed = if !status.conflicted.is_empty() {
                RepoState::Conflict
            } else if RepoState::is_dirty(&status) {
                RepoState::Dirty
            } else if DivergenceMeaning::Both.is_diverged(ahead, behind) {
                RepoState::Diverged
            } else if behind > 0 {
                RepoState::Unpulled
            } else if ahead > 0 {
                RepoState::Unpushed
            } else {
                RepoState::Clean
            };
            assert_eq!(
                RepoState::from_root(&root(status.clone()), ahead, behind),
                composed,
                "{case}"
            );
        }
    }
}
