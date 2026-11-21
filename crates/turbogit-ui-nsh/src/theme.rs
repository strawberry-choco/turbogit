//! The data token layer: the roles the chrome tokens in `theme.json` have no
//! field for. `nsh_base::theme()` reads the chrome roles (surfaces, ink,
//! primary, accent); this module owns diff color, file status color, state
//! color, repository-state color, ref accents, lanes, and the type scale,
//! density, and radii the controls size themselves by.
//!
//! Every color is an [`AdaptiveColor`]: the dark value carries the role's
//! egui value (`crates/turbogit-ui/src/theme.rs`), the light value is its
//! honest lighter counterpart. The redesign is nsh-native, not pixel parity,
//! but the roles and their relationships (staged is the foreground pair,
//! unstaged the muted pair, state is colored text) carry over.

use nsh_core::prelude::*;

/// Diff: added lines, staged. The foreground pair — a staged row wears it.
pub const DIFF_ADD_BG: AdaptiveColor = AdaptiveColor::rgb(0xcfe6d4, 0x344f3e);
pub const DIFF_ADD_TEXT: AdaptiveColor = AdaptiveColor::rgb(0x1a5e2f, 0x85e89d);

/// Diff: deleted lines, staged. The foreground pair's delete side.
pub const DIFF_DEL_BG: AdaptiveColor = AdaptiveColor::rgb(0xf2d4d4, 0x5a3a3a);
pub const DIFF_DEL_TEXT: AdaptiveColor = AdaptiveColor::rgb(0x8c2f2f, 0xff9a9a);

/// Diff accents: the one-green-two-names identity `STATUS_ADDED` shares.
pub const DIFF_ADD_ACCENT: AdaptiveColor = AdaptiveColor::rgb(0x276f33, 0x6fae75);
pub const DIFF_DEL_ACCENT: AdaptiveColor = AdaptiveColor::rgb(0xcc3a49, 0xf75464);

/// Diff: the muted pair an unstaged row wears.
pub const DIFF_ADD_UNSTAGED_BG: AdaptiveColor = AdaptiveColor::rgb(0xe2efe4, 0x2a3631);
pub const DIFF_ADD_UNSTAGED_TEXT: AdaptiveColor = DIFF_ADD_ACCENT;
pub const DIFF_DEL_UNSTAGED_BG: AdaptiveColor = AdaptiveColor::rgb(0xeddfe1, 0x392d30);
pub const DIFF_DEL_UNSTAGED_TEXT: AdaptiveColor = AdaptiveColor::rgb(0x9c4747, 0xd98383);

/// File status: modified, added, unversioned, deleted (the muted step).
pub const STATUS_MODIFIED: AdaptiveColor = AdaptiveColor::rgb(0x33578f, 0xa8c0e8);
pub const STATUS_ADDED: AdaptiveColor = AdaptiveColor::rgb(0x276f33, 0x6fae75);
pub const STATUS_UNVERSIONED: AdaptiveColor = AdaptiveColor::rgb(0x7a7748, 0xb5b37e);
pub const STATUS_DELETED: AdaptiveColor = AdaptiveColor::rgb(0x5d636d, 0x8a8e96);

/// State color: success, warning, error, info.
pub const STATE_SUCCESS: AdaptiveColor = AdaptiveColor::rgb(0x2e7d3a, 0x4caf50);
pub const STATE_WARNING: AdaptiveColor = AdaptiveColor::rgb(0xb97610, 0xf9a825);
pub const STATE_ERROR: AdaptiveColor = AdaptiveColor::rgb(0xd8403d, 0xef5350);
pub const STATE_INFO: AdaptiveColor = AdaptiveColor::rgb(0x2a72c4, 0x42a5f5);

/// Ref and accent color: branch ahead, danger, cascade accent, counter, link,
/// accent text.
pub const AHEAD: AdaptiveColor = AdaptiveColor::rgb(0x2e7d3a, 0x5fa86c);
pub const DANGER: AdaptiveColor = AdaptiveColor::rgb(0xc33c3c, 0xdb5c5c);
pub const CASCADE_ACCENT: AdaptiveColor = AdaptiveColor::rgb(0x6d4fd4, 0xa78bfa);
pub const COUNTER: AdaptiveColor = AdaptiveColor::rgb(0xb36620, 0xe0883c);
pub const LINK: AdaptiveColor = AdaptiveColor::rgb(0x2a63c9, 0x74a3e8);
pub const ACCENT_TEXT: AdaptiveColor = AdaptiveColor::rgb(0x2c5ec4, 0x8bb5f5);

/// The graph's lane rails, one color per lane column.
pub const LANES: [AdaptiveColor; 4] = [
    AdaptiveColor::rgb(0x2a63c9, 0x3574f0),
    AdaptiveColor::rgb(0x2e7d3a, 0x4caf50),
    AdaptiveColor::rgb(0xb97610, 0xf9a825),
    AdaptiveColor::rgb(0x6d4fd4, 0xa78bfa),
];

/// The lane color of lane column `at`.
pub fn lane(at: usize) -> AdaptiveColor {
    LANES[at % LANES.len()]
}

/// How much a section band tints its surface, and a marker its row.
pub const SECTION_TINT: f32 = 0.12;
pub const MARKER_TINT: f32 = 0.15;

/// Which state a repository is in: the seven states the egui token layer
/// names. The verdict ladder that resolves a root into one of these belongs
/// to the status surface that needs it; this module owns the paint only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// The one spelling of each state, every surface reads from here.
    pub fn words(self) -> &'static str {
        match self {
            RepoState::Clean => "",
            RepoState::Dirty => "dirty",
            RepoState::Conflict => "has conflicts",
            RepoState::Diverged => "diverged",
            RepoState::Unpushed => "unpushed commits",
            RepoState::Unpulled => "unpulled commits",
            RepoState::Uninitialized => "not initialized",
        }
    }

    /// The state's color. Three severities name the map (`AHEAD`, `COUNTER`,
    /// `STATE_ERROR`); uninitialized is work the user is owed, so it wears
    /// the muted ink and the app invents no fourth tone.
    pub fn color(self) -> AdaptiveColor {
        match self {
            RepoState::Clean | RepoState::Unpushed => AHEAD,
            RepoState::Dirty | RepoState::Unpulled => COUNTER,
            RepoState::Conflict | RepoState::Diverged => STATE_ERROR,
            RepoState::Uninitialized => AdaptiveColor::rgb(0x5d636d, 0x8a8e96),
        }
    }
}

/// The type scale, in points: nine named steps, the chrome face reads them.
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

/// Density: the paddings, spacings, and row heights the controls lay out by.
pub const PANEL_PADDING: f32 = 12.0;
pub const SPACING_X: f32 = 8.0;
pub const SPACING_Y: f32 = 6.0;
pub const BUTTON_PAD_X: f32 = 10.0;
pub const BUTTON_PAD_Y: f32 = 5.0;
pub const INDENT: f32 = 14.0;
pub const CELL_GAP: f32 = 8.0;

pub const DENSITY_COMPACT_BUTTON_PAD_X: f32 = 8.0;
pub const DENSITY_COMPACT_BUTTON_PAD_Y: f32 = 4.0;
pub const DENSITY_DENSE_BUTTON_PAD_X: f32 = 6.0;
pub const DENSITY_DENSE_BUTTON_PAD_Y: f32 = 2.0;
pub const DENSITY_COMMAND_BUTTON_PAD_X: f32 = 8.0;
pub const DENSITY_COMMAND_BUTTON_PAD_Y: f32 = 1.0;
pub const DENSITY_COMMAND_ROW_HEIGHT: f32 = 14.0;
pub const DENSITY_LIST_GAP: f32 = 2.0;

/// Row heights: a file row and a group band in a list or tree.
pub const FILE_ROW_HEIGHT: f32 = 24.0;
pub const GROUP_ROW_HEIGHT: f32 = 26.0;

/// Radii per role. The chip keeps its compact corner, the badge its pill;
/// neither merges with the other.
pub const CHIP_RADIUS: f32 = 3.0;
pub const CONTROL_RADIUS: f32 = 4.0;
pub const WINDOW_RADIUS: f32 = 8.0;
pub const MENU_RADIUS: f32 = 6.0;
pub const CARD_RADIUS: f32 = 8.0;
pub const PILL_RADIUS: f32 = 9.0;
pub const MARK_RADIUS: f32 = 2.0;

/// The selection rail's width, and the text caret's. Two separate tokens:
/// the rail means "this row is chosen", the caret means where the reader is.
pub const RAIL_WIDTH: f32 = 2.0;
pub const CARET_WIDTH: f32 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: nsh_core::Hsla) -> f64 {
        let rgba = nsh_core::Rgba::from(color);
        let channel = |c: f32| {
            let c = c as f64;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgba.r) + 0.7152 * channel(rgba.g) + 0.0722 * channel(rgba.b)
    }

    fn contrast(a: nsh_core::Hsla, b: nsh_core::Hsla) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (brighter, dimmer) = if la > lb { (la, lb) } else { (lb, la) };
        (brighter + 0.05) / (dimmer + 0.05)
    }

    /// Every ink-on-band pair the diff roles name must clear AA in both
    /// appearances: an ink change alone cannot clear AA for running text, so
    /// each band and its ink are one contract.
    #[test]
    fn diff_ink_pairs_clear_aa_on_their_bands_in_both_appearances() {
        let pairs = [
            ("staged add", DIFF_ADD_TEXT, DIFF_ADD_BG),
            ("staged del", DIFF_DEL_TEXT, DIFF_DEL_BG),
            ("unstaged add", DIFF_ADD_UNSTAGED_TEXT, DIFF_ADD_UNSTAGED_BG),
            ("unstaged del", DIFF_DEL_UNSTAGED_TEXT, DIFF_DEL_UNSTAGED_BG),
        ];
        for (name, ink, bg) in pairs {
            for (appearance, surface) in [("light", bg.light), ("dark", bg.dark)] {
                let ratio = contrast(
                    if appearance == "light" {
                        ink.light
                    } else {
                        ink.dark
                    },
                    surface,
                );
                assert!(
                    ratio >= 4.5,
                    "{name} ink on its {appearance} band must clear AA: {ratio:.3}:1"
                );
            }
        }
    }

    /// The unstaged pair is the muted one: its inks read weaker than the
    /// staged pair's on the same background, so stagedness survives a glance.
    #[test]
    fn unstaged_ink_reads_weaker_than_staged_ink_on_the_app_background() {
        for appearance in ["light", "dark"] {
            let (staged, unstaged) = if appearance == "light" {
                (DIFF_ADD_TEXT.light, DIFF_ADD_UNSTAGED_TEXT.light)
            } else {
                (DIFF_ADD_TEXT.dark, DIFF_ADD_UNSTAGED_TEXT.dark)
            };
            let card = if appearance == "light" {
                nsh_core::Hsla::parse_hex("#ffffff").unwrap()
            } else {
                nsh_core::Hsla::parse_hex("#2b2d30").unwrap()
            };
            let staged_ratio = contrast(staged, card);
            let unstaged_ratio = contrast(unstaged, card);
            assert!(
                staged_ratio > unstaged_ratio,
                "staged add ink must read stronger than unstaged on the card in \
                 {appearance}: {staged_ratio:.3}:1 vs {unstaged_ratio:.3}:1"
            );
        }
    }

    /// The repository-state map paints exactly three severities plus the
    /// muted ink; no state invents a fourth tone.
    #[test]
    fn repo_state_colors_come_from_three_severities_and_muted() {
        let severities = [AHEAD, COUNTER, STATE_ERROR];
        let muted = AdaptiveColor::rgb(0x5d636d, 0x8a8e96);
        for state in [
            RepoState::Clean,
            RepoState::Dirty,
            RepoState::Conflict,
            RepoState::Diverged,
            RepoState::Unpushed,
            RepoState::Unpulled,
            RepoState::Uninitialized,
        ] {
            let color = state.color();
            assert!(
                severities.contains(&color) || color == muted,
                "{state:?} wears a tone outside the severity table and muted"
            );
        }
    }

    /// Every lane column resolves to one of the four rails, forever.
    #[test]
    fn lane_resolves_every_column_index() {
        for at in [0, 1, 2, 3, 4, 7, 12, 999] {
            let _ = lane(at);
        }
        assert_eq!(LANES.len(), 4);
    }
}
