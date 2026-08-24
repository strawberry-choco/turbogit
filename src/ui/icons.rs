//! Lucide icon primitives (issue #7, spec §5).
//!
//! Every icon is embedded as Lucide SVG path data (24×24 viewBox, 2px
//! strokes) and rendered as stroked egui primitives scaled by `size / 24`
//! (spec §5.1/§5.3). Lookup is by name ([`Icon::from_name`]); unknown names
//! log at debug level and paint nothing — never panic.

use std::collections::HashMap;
use std::sync::OnceLock;

use egui::epaint::PathShape;
use egui::{Color32, Pos2, Shape, Stroke, Ui, Vec2};

/// Typed handle to one of the embedded Lucide icons (spec §5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Icon {
    name: &'static str,
}

impl Icon {
    // --- GENERATED icon constants (spec §5.2) ---

    pub const ALERT_CIRCLE: Self = Self {
        name: "alert-circle",
    };
    pub const ALERT_TRIANGLE: Self = Self {
        name: "alert-triangle",
    };
    pub const ALIGN_JUSTIFY: Self = Self {
        name: "align-justify",
    };
    pub const ARCHIVE: Self = Self { name: "archive" };
    pub const ARROW_DOWN: Self = Self { name: "arrow-down" };
    pub const ARROW_DOWN_CIRCLE: Self = Self {
        name: "arrow-down-circle",
    };
    pub const ARROW_LEFT: Self = Self { name: "arrow-left" };
    pub const ARROW_RIGHT: Self = Self {
        name: "arrow-right",
    };
    pub const ARROW_RIGHT_LEFT: Self = Self {
        name: "arrow-right-left",
    };
    pub const ARROW_UP: Self = Self { name: "arrow-up" };
    pub const BELL: Self = Self { name: "bell" };
    pub const BOOK_OPEN: Self = Self { name: "book-open" };
    pub const BUG: Self = Self { name: "bug" };
    pub const CHECK: Self = Self { name: "check" };
    pub const CHECK_SQUARE: Self = Self {
        name: "check-square",
    };
    pub const CHEVRON_DOWN: Self = Self {
        name: "chevron-down",
    };
    pub const CHEVRON_LEFT: Self = Self {
        name: "chevron-left",
    };
    pub const CHEVRON_RIGHT: Self = Self {
        name: "chevron-right",
    };
    pub const CHEVRON_UP: Self = Self { name: "chevron-up" };
    pub const CLOCK: Self = Self { name: "clock" };
    pub const COLUMNS: Self = Self { name: "columns" };
    pub const DOWNLOAD: Self = Self { name: "download" };
    pub const EYE_OFF: Self = Self { name: "eye-off" };
    pub const FILE: Self = Self { name: "file" };
    pub const FILE_CODE: Self = Self { name: "file-code" };
    pub const FILE_MINUS: Self = Self { name: "file-minus" };
    pub const FILE_PLUS: Self = Self { name: "file-plus" };
    pub const FILE_WARNING: Self = Self {
        name: "file-warning",
    };
    pub const FILES: Self = Self { name: "files" };
    pub const FILTER: Self = Self { name: "filter" };
    pub const FOLDER: Self = Self { name: "folder" };
    pub const FOLDER_GIT: Self = Self { name: "folder-git" };
    pub const FOLDER_OPEN: Self = Self {
        name: "folder-open",
    };
    pub const GIT_BRANCH: Self = Self { name: "git-branch" };
    pub const GIT_COMMIT: Self = Self { name: "git-commit" };
    pub const GIT_COMPARE: Self = Self {
        name: "git-compare",
    };
    pub const GIT_MERGE: Self = Self { name: "git-merge" };
    pub const KEYBOARD: Self = Self { name: "keyboard" };
    pub const LAPTOP: Self = Self { name: "laptop" };
    pub const LAYERS: Self = Self { name: "layers" };
    pub const LAYOUT: Self = Self { name: "layout" };
    pub const MENU: Self = Self { name: "menu" };
    pub const MONITOR: Self = Self { name: "monitor" };
    pub const MORE_HORIZONTAL: Self = Self {
        name: "more-horizontal",
    };
    pub const PLAY: Self = Self { name: "play" };
    pub const PLUS: Self = Self { name: "plus" };
    pub const PLUS_CIRCLE: Self = Self {
        name: "plus-circle",
    };
    pub const REFRESH_CW: Self = Self { name: "refresh-cw" };
    pub const SEARCH: Self = Self { name: "search" };
    pub const SETTINGS: Self = Self { name: "settings" };
    pub const STAR: Self = Self { name: "star" };
    pub const TAG: Self = Self { name: "tag" };
    pub const TRASH_2: Self = Self { name: "trash-2" };
    pub const UNDO: Self = Self { name: "undo" };
    pub const UPLOAD: Self = Self { name: "upload" };
    pub const X: Self = Self { name: "x" };

    /// All embedded icons, in spec §5.2 order.
    pub const ALL: &'static [Self] = &[
        Self::ALERT_CIRCLE,
        Self::ALERT_TRIANGLE,
        Self::ALIGN_JUSTIFY,
        Self::ARCHIVE,
        Self::ARROW_DOWN,
        Self::ARROW_DOWN_CIRCLE,
        Self::ARROW_LEFT,
        Self::ARROW_RIGHT,
        Self::ARROW_RIGHT_LEFT,
        Self::ARROW_UP,
        Self::BELL,
        Self::BOOK_OPEN,
        Self::BUG,
        Self::CHECK,
        Self::CHECK_SQUARE,
        Self::CHEVRON_DOWN,
        Self::CHEVRON_LEFT,
        Self::CHEVRON_RIGHT,
        Self::CHEVRON_UP,
        Self::CLOCK,
        Self::COLUMNS,
        Self::DOWNLOAD,
        Self::EYE_OFF,
        Self::FILE,
        Self::FILE_CODE,
        Self::FILE_MINUS,
        Self::FILE_PLUS,
        Self::FILE_WARNING,
        Self::FILES,
        Self::FILTER,
        Self::FOLDER,
        Self::FOLDER_GIT,
        Self::FOLDER_OPEN,
        Self::GIT_BRANCH,
        Self::GIT_COMMIT,
        Self::GIT_COMPARE,
        Self::GIT_MERGE,
        Self::KEYBOARD,
        Self::LAPTOP,
        Self::LAYERS,
        Self::LAYOUT,
        Self::MENU,
        Self::MONITOR,
        Self::MORE_HORIZONTAL,
        Self::PLAY,
        Self::PLUS,
        Self::PLUS_CIRCLE,
        Self::REFRESH_CW,
        Self::SEARCH,
        Self::SETTINGS,
        Self::STAR,
        Self::TAG,
        Self::TRASH_2,
        Self::UNDO,
        Self::UPLOAD,
        Self::X,
    ];
    // --- END GENERATED CONSTANTS ---

    /// The Lucide name (`"check"`), i.e. the lookup key.
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Lookup by Lucide name; `None` when not in the embedded set.
    pub fn from_name(name: &str) -> Option<Self> {
        ICON_PATHS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(n, _)| Self { name: n })
    }
}

/// Embedded Lucide SVG path data (spec §5.1): `(name, [subpath "d" strings])`,
/// generated from the official set. 24×24 viewBox, 2px strokes.
#[rustfmt::skip]
static ICON_PATHS: &[(&str, &[&str])] = &[
    ("alert-circle", &["M 2 12 A 10 10 0 1 0 22 12 A 10 10 0 1 0 2 12", "M 12 8 L 12 12", "M 12 16 L 12.01 16"]),
    ("alert-triangle", &["m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4", "M12 17h.01"]),
    ("align-justify", &["M 3 6 L 21 6", "M 3 12 L 21 12", "M 3 18 L 21 18"]),
    ("archive", &["M 3 3 H 21 A 1 1 0 0 1 22 4 V 7 A 1 1 0 0 1 21 8 H 3 A 1 1 0 0 1 2 7 V 4 A 1 1 0 0 1 3 3 Z", "M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8", "M10 12h4"]),
    ("arrow-down", &["M12 5v14", "m19 12-7 7-7-7"]),
    ("arrow-down-circle", &["M 2 12 A 10 10 0 1 0 22 12 A 10 10 0 1 0 2 12", "M12 8v8", "m8 12 4 4 4-4"]),
    ("arrow-left", &["m12 19-7-7 7-7", "M19 12H5"]),
    ("arrow-right", &["M5 12h14", "m12 5 7 7-7 7"]),
    ("arrow-right-left", &["m16 3 4 4-4 4", "M20 7H4", "m8 21-4-4 4-4", "M4 17h16"]),
    ("arrow-up", &["m5 12 7-7 7 7", "M12 19V5"]),
    ("bell", &["M10.268 21a2 2 0 0 0 3.464 0", "M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"]),
    ("book-open", &["M12 5v16", "M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z"]),
    ("bug", &["M12 20v-9", "M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4z", "M14.12 3.88 16 2", "M21 21a4 4 0 0 0-3.81-4", "M21 5a4 4 0 0 1-3.55 3.97", "M22 13h-4", "M3 21a4 4 0 0 1 3.81-4", "M3 5a4 4 0 0 0 3.55 3.97", "M6 13H2", "m8 2 1.88 1.88", "M9 7.13V6a3 3 0 1 1 6 0v1.13"]),
    ("check", &["M20 6 9 17l-5-5"]),
    ("check-square", &["M 5 3 H 19 A 2 2 0 0 1 21 5 V 19 A 2 2 0 0 1 19 21 H 5 A 2 2 0 0 1 3 19 V 5 A 2 2 0 0 1 5 3 Z", "m9 12 2 2 4-4"]),
    ("chevron-down", &["m6 9 6 6 6-6"]),
    ("chevron-left", &["m15 18-6-6 6-6"]),
    ("chevron-right", &["m9 18 6-6-6-6"]),
    ("chevron-up", &["m18 15-6-6-6 6"]),
    ("clock", &["M 2 12 A 10 10 0 1 0 22 12 A 10 10 0 1 0 2 12", "M12 6v6l4 2"]),
    ("columns", &["M 5 3 H 19 A 2 2 0 0 1 21 5 V 19 A 2 2 0 0 1 19 21 H 5 A 2 2 0 0 1 3 19 V 5 A 2 2 0 0 1 5 3 Z", "M12 3v18"]),
    ("download", &["M12 15V3", "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4", "m7 10 5 5 5-5"]),
    ("eye-off", &["M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49", "M14.084 14.158a3 3 0 0 1-4.242-4.242", "M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143", "m2 2 20 20"]),
    ("file", &["M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", "M14 2v5a1 1 0 0 0 1 1h5"]),
    ("file-code", &["M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", "M14 2v5a1 1 0 0 0 1 1h5", "M10 12.5 8 15l2 2.5", "m14 12.5 2 2.5-2 2.5"]),
    ("file-minus", &["M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", "M14 2v5a1 1 0 0 0 1 1h5", "M9 15h6"]),
    ("file-plus", &["M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", "M14 2v5a1 1 0 0 0 1 1h5", "M9 15h6", "M12 18v-6"]),
    ("file-warning", &["M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z", "M12 9v4", "M12 17h.01"]),
    ("files", &["M15 2h-4a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V8", "M16.706 2.706A2.4 2.4 0 0 0 15 2v5a1 1 0 0 0 1 1h5a2.4 2.4 0 0 0-.706-1.706z", "M5 7a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8a2 2 0 0 0 1.732-1"]),
    ("filter", &["M 22 3 L 2 3 L 10 12.46 L 10 19 L 14 21 L 14 12.46 L 22 3 Z"]),
    ("folder", &["M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"]),
    ("folder-git", &["M 10 13 A 2 2 0 1 0 14 13 A 2 2 0 1 0 10 13", "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z", "M14 13h3", "M7 13h3"]),
    ("folder-open", &["m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"]),
    ("git-branch", &["M15 6a9 9 0 0 0-9 9V3", "M 15 6 A 3 3 0 1 0 21 6 A 3 3 0 1 0 15 6", "M 3 18 A 3 3 0 1 0 9 18 A 3 3 0 1 0 3 18"]),
    ("git-commit", &["M 9 12 A 3 3 0 1 0 15 12 A 3 3 0 1 0 9 12", "M 3 12 L 9 12", "M 15 12 L 21 12"]),
    ("git-compare", &["M 15 18 A 3 3 0 1 0 21 18 A 3 3 0 1 0 15 18", "M 3 6 A 3 3 0 1 0 9 6 A 3 3 0 1 0 3 6", "M13 6h3a2 2 0 0 1 2 2v7", "M11 18H8a2 2 0 0 1-2-2V9"]),
    ("git-merge", &["M 15 18 A 3 3 0 1 0 21 18 A 3 3 0 1 0 15 18", "M 3 6 A 3 3 0 1 0 9 6 A 3 3 0 1 0 3 6", "M6 21V9a9 9 0 0 0 9 9"]),
    ("keyboard", &["M10 8h.01", "M12 12h.01", "M14 8h.01", "M16 12h.01", "M18 8h.01", "M6 8h.01", "M7 16h10", "M8 12h.01", "M 4 4 H 20 A 2 2 0 0 1 22 6 V 18 A 2 2 0 0 1 20 20 H 4 A 2 2 0 0 1 2 18 V 6 A 2 2 0 0 1 4 4 Z"]),
    ("laptop", &["M18 5a2 2 0 0 1 2 2v8.526a2 2 0 0 0 .212.897l1.068 2.127a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45l1.068-2.127A2 2 0 0 0 4 15.526V7a2 2 0 0 1 2-2z", "M20.054 15.987H3.946"]),
    ("layers", &["M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z", "M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12", "M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17"]),
    ("layout", &["M 5 3 H 19 A 2 2 0 0 1 21 5 V 19 A 2 2 0 0 1 19 21 H 5 A 2 2 0 0 1 3 19 V 5 A 2 2 0 0 1 5 3 Z", "M 3 9 L 21 9", "M 9 21 L 9 9"]),
    ("menu", &["M4 5h16", "M4 12h16", "M4 19h16"]),
    ("monitor", &["M 4 3 H 20 A 2 2 0 0 1 22 5 V 15 A 2 2 0 0 1 20 17 H 4 A 2 2 0 0 1 2 15 V 5 A 2 2 0 0 1 4 3 Z", "M 8 21 L 16 21", "M 12 17 L 12 21"]),
    ("more-horizontal", &["M 11 12 A 1 1 0 1 0 13 12 A 1 1 0 1 0 11 12", "M 18 12 A 1 1 0 1 0 20 12 A 1 1 0 1 0 18 12", "M 4 12 A 1 1 0 1 0 6 12 A 1 1 0 1 0 4 12"]),
    ("play", &["M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"]),
    ("plus", &["M5 12h14", "M12 5v14"]),
    ("plus-circle", &["M 2 12 A 10 10 0 1 0 22 12 A 10 10 0 1 0 2 12", "M8 12h8", "M12 8v8"]),
    ("refresh-cw", &["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"]),
    ("search", &["m21 21-4.34-4.34", "M 3 11 A 8 8 0 1 0 19 11 A 8 8 0 1 0 3 11"]),
    ("settings", &["M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915", "M 9 12 A 3 3 0 1 0 15 12 A 3 3 0 1 0 9 12"]),
    ("star", &["M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"]),
    ("tag", &["M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z", "M 7 7.5 A 0.5 0.5 0 1 0 8 7.5 A 0.5 0.5 0 1 0 7 7.5"]),
    ("trash-2", &["M10 11v6", "M14 11v6", "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6", "M3 6h18", "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"]),
    ("undo", &["M3 7v6h6", "M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"]),
    ("upload", &["M12 3v12", "m17 8-5-5-5 5", "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"]),
    ("x", &["M18 6 6 18", "m6 6 12 12"]),
];
/// Paint `name` at the cursor, scaled to `size × size`, stroked with `color`.
///
/// Unknown names paint nothing (and log at debug) — never panic (§5.3).
pub fn icon(ui: &mut Ui, name: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_at(ui.painter(), rect.min, size, name.name(), color);
}

/// Paint `name` looked up by string; misses are safe no-ops.
pub fn icon_by_name(ui: &mut Ui, name: &str, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_at(ui.painter(), rect.min, size, name, color);
}

/// Paint an icon's strokes into `rect`'s top-left at `size`, stroked `color`.
fn paint_at(painter: &egui::Painter, origin: Pos2, size: f32, name: &str, color: Color32) {
    let Some(subpaths) = geometry(name) else {
        // Spec §5.3: missing icons render nothing and never panic.
        log::debug!(target: "turbogit::icons", "unknown icon name `{name}`");
        return;
    };
    let scale = size / 24.0;
    let stroke = Stroke::new(2.0 * scale, color);
    for (points, closed) in subpaths {
        let mapped: Vec<Pos2> = points
            .iter()
            .map(|p| origin + p.to_vec2() * scale)
            .collect();
        painter.add(Shape::Path(PathShape {
            points: mapped,
            closed: *closed,
            fill: Color32::TRANSPARENT,
            stroke: stroke.into(),
        }));
    }
}

// --- Geometry: embedded path data → flattened polylines ---

/// Flattened geometry: one polyline (+ closed flag) per SVG subpath.
type Subpaths = Vec<(Vec<Pos2>, bool)>;

/// Parsed polylines per icon, computed once on first use.
fn geometry(name: &str) -> Option<&'static Subpaths> {
    static CACHE: OnceLock<HashMap<&'static str, Subpaths>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| {
        ICON_PATHS
            .iter()
            .map(|(name, ds)| (*name, parse_all(ds)))
            .collect()
    });
    cache.get(name)
}

fn parse_all(ds: &[&str]) -> Subpaths {
    let mut out = Vec::new();
    for d in ds {
        parse_d(d, &mut out);
    }
    out
}

// --- Minimal SVG path-data parser (linear commands) ---

enum Tok {
    Cmd(u8),
    Num(f32),
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    /// Scan one token without consuming it.
    ///
    /// `next` only ever advances `pos`, so saving/restoring it is a valid
    /// way to look ahead.
    fn peek(&mut self) -> Option<Tok> {
        let save = self.pos;
        let tok = self.next();
        self.pos = save;
        tok
    }

    fn next(&mut self) -> Option<Tok> {
        while matches!(self.bytes.get(self.pos), Some(c) if c.is_ascii_whitespace() || *c == b',') {
            self.pos += 1;
        }
        let c = *self.bytes.get(self.pos)?;
        if c.is_ascii_alphabetic() {
            self.pos += 1;
            return Some(Tok::Cmd(c));
        }
        // Number: [-+]?digits[.digits][eE[-+]digits] — '-' starts a new number.
        let start = self.pos;
        if c == b'-' || c == b'+' {
            self.pos += 1;
        }
        let mut seen_digit = false;
        while matches!(self.bytes.get(self.pos), Some(d) if d.is_ascii_digit()) {
            seen_digit = true;
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            while matches!(self.bytes.get(self.pos), Some(d) if d.is_ascii_digit()) {
                seen_digit = true;
                self.pos += 1;
            }
        }
        if !seen_digit {
            return None; // malformed; stop parsing this path
        }
        if matches!(self.bytes.get(self.pos), Some(b'e') | Some(b'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'-') | Some(b'+')) {
                self.pos += 1;
            }
            let mut exp_digits = false;
            while matches!(self.bytes.get(self.pos), Some(d) if d.is_ascii_digit()) {
                exp_digits = true;
                self.pos += 1;
            }
            if !exp_digits {
                self.pos = save;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        text.parse::<f32>().ok().map(Tok::Num)
    }

    fn number(&mut self) -> Option<f32> {
        match self.next()? {
            Tok::Num(v) => Some(v),
            Tok::Cmd(_) => None,
        }
    }

    /// One arc flag: exactly a single `0`/`1`, which SVG allows to run
    /// together with following numbers (`a2 2 0 0022 17`).
    fn flag(&mut self) -> Option<bool> {
        while matches!(self.bytes.get(self.pos), Some(c) if c.is_ascii_whitespace() || *c == b',') {
            self.pos += 1;
        }
        match self.bytes.get(self.pos)? {
            b'0' => {
                self.pos += 1;
                Some(false)
            }
            b'1' => {
                self.pos += 1;
                Some(true)
            }
            _ => None,
        }
    }
}

/// Parse one `d` string into open/closed polylines. Malformed input stops
/// parsing early but never panics; whatever was parsed so far still renders.
fn parse_d(d: &str, out: &mut Subpaths) {
    let mut lx = Lexer::new(d);
    let mut cur = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    let mut pts: Vec<Pos2> = Vec::new();
    let mut last_cmd = b'\0';
    // Control points of the previous curve, for S/T smooth reflection.
    let mut last_ctrl = Pos2::ZERO;
    let mut last_quad_ctrl = Pos2::ZERO;

    loop {
        // A command letter — or a bare number, which repeats the previous
        // command (implicit repetition: M→L, m→l).
        let cmd = match lx.peek() {
            Some(Tok::Cmd(c)) => {
                lx.next();
                c
            }
            Some(Tok::Num(_)) => match last_cmd {
                0 => break, // number before any command
                b'M' => b'L',
                b'm' => b'l',
                c => c,
            },
            None => break,
        };
        macro_rules! coord {
            () => {
                match lx.number() {
                    Some(v) => v,
                    None => break, // truncated input
                }
            };
        }
        // Ensure the subpath has an anchor point before adding segments.
        macro_rules! anchored {
            () => {
                if pts.is_empty() {
                    pts.push(cur);
                }
            };
        }

        match cmd {
            b'M' | b'm' => {
                if !pts.is_empty() {
                    flush(&mut pts, out);
                }
                let (x, y) = (coord!(), coord!());
                cur = if cmd == b'm' {
                    cur + Vec2::new(x, y)
                } else {
                    pos2(x, y)
                };
                start = cur;
                pts.push(cur);
            }
            b'L' | b'l' | b'H' | b'h' | b'V' | b'v' => {
                anchored!();
                match cmd {
                    b'L' => cur = pos2(coord!(), coord!()),
                    b'l' => cur += Vec2::new(coord!(), coord!()),
                    b'H' => cur.x = coord!(),
                    b'h' => cur.x += coord!(),
                    b'V' => cur.y = coord!(),
                    _ => cur.y += coord!(),
                }
                pts.push(cur);
            }
            b'C' | b'c' | b'S' | b's' => {
                anchored!();
                let (p1, p2, p3) = if cmd == b'C' || cmd == b'c' {
                    let (x1, y1, x2, y2, x, y) =
                        (coord!(), coord!(), coord!(), coord!(), coord!(), coord!());
                    let rel = cmd == b'c';
                    let off = |a: f32, b: f32| if rel { a + b } else { b };
                    (
                        pos2(off(cur.x, x1), off(cur.y, y1)),
                        pos2(off(cur.x, x2), off(cur.y, y2)),
                        pos2(off(cur.x, x), off(cur.y, y)),
                    )
                } else {
                    let (x2, y2, x, y) = (coord!(), coord!(), coord!(), coord!());
                    let rel = cmd == b's';
                    let off = |a: f32, b: f32| if rel { a + b } else { b };
                    // First control reflects the previous curve's second control.
                    let p1 = if matches!(last_cmd, b'C' | b'S' | b'c' | b's') {
                        pos2(2.0 * cur.x - last_ctrl.x, 2.0 * cur.y - last_ctrl.y)
                    } else {
                        cur
                    };
                    (
                        p1,
                        pos2(off(cur.x, x2), off(cur.y, y2)),
                        pos2(off(cur.x, x), off(cur.y, y)),
                    )
                };
                sample_cubic(&mut pts, cur, p1, p2, p3);
                last_ctrl = p2;
                cur = p3;
            }
            b'Q' | b'q' | b'T' | b't' => {
                anchored!();
                let (p1, p2) = if cmd == b'Q' || cmd == b'q' {
                    let (x1, y1, x, y) = (coord!(), coord!(), coord!(), coord!());
                    let rel = cmd == b'q';
                    let off = |a: f32, b: f32| if rel { a + b } else { b };
                    (
                        pos2(off(cur.x, x1), off(cur.y, y1)),
                        pos2(off(cur.x, x), off(cur.y, y)),
                    )
                } else {
                    let (x, y) = (coord!(), coord!());
                    let rel = cmd == b't';
                    let end = if rel {
                        cur + Vec2::new(x, y)
                    } else {
                        pos2(x, y)
                    };
                    // Control reflects the previous quadratic's control.
                    let ctrl = if matches!(last_cmd, b'Q' | b'T' | b'q' | b't') {
                        pos2(
                            2.0 * cur.x - last_quad_ctrl.x,
                            2.0 * cur.y - last_quad_ctrl.y,
                        )
                    } else {
                        cur
                    };
                    (ctrl, end)
                };
                sample_quad(&mut pts, cur, p1, p2);
                last_quad_ctrl = p1;
                cur = p2;
            }
            b'A' | b'a' => {
                anchored!();
                let rx = coord!();
                let ry = coord!();
                let rot = coord!();
                let (Some(large_arc), Some(sweep)) = (lx.flag(), lx.flag()) else {
                    break; // malformed flags
                };
                let (x, y) = (coord!(), coord!());
                let to = if cmd == b'A' {
                    pos2(x, y)
                } else {
                    cur + Vec2::new(x, y)
                };
                push_arc(&mut pts, cur, rx, ry, rot, large_arc, sweep, to);
                cur = to;
            }
            b'Z' | b'z' => {
                if pts.len() >= 2 {
                    out.push((std::mem::take(&mut pts), true));
                } else {
                    pts.clear();
                }
                cur = start;
            }
            _ => break, // unknown command: keep what parsed so far
        }
        last_cmd = cmd;
    }
    flush(&mut pts, out);
}

/// Sample a cubic Bézier into `pts` (start point already present).
fn sample_cubic(pts: &mut Vec<Pos2>, p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) {
    const N: usize = 12;
    for i in 1..=N {
        let t = i as f32 / N as f32;
        let u = 1.0 - t;
        pts.push(pos2(
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
        ));
    }
}

/// Sample a quadratic Bézier into `pts` (start point already present).
fn sample_quad(pts: &mut Vec<Pos2>, p0: Pos2, p1: Pos2, p2: Pos2) {
    const N: usize = 8;
    for i in 1..=N {
        let t = i as f32 / N as f32;
        let u = 1.0 - t;
        pts.push(pos2(
            u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
            u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
        ));
    }
}

/// Append an elliptical arc from `from` to `to` as sampled points
/// (SVG endpoint→center parameterization, W3C SVG 1.1 §F.6).
#[allow(clippy::too_many_arguments)] // mirrors the SVG arc parameterization
fn push_arc(
    pts: &mut Vec<Pos2>,
    from: Pos2,
    rx_in: f32,
    ry_in: f32,
    phi_deg: f32,
    large_arc: bool,
    sweep: bool,
    to: Pos2,
) {
    let (rx, ry) = (rx_in.abs(), ry_in.abs());
    if rx <= f32::EPSILON || ry <= f32::EPSILON || from.distance(to) < f32::EPSILON {
        pts.push(to); // degenerate arc renders as a line
        return;
    }
    let phi = phi_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    // F.6.5.1 — endpoint delta in the rotated frame.
    let dx2 = (from.x - to.x) / 2.0;
    let dy2 = (from.y - to.y) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    // F.6.5.2 — scale radii up when the arc is out of range.
    let lambda = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    let (rx, ry) = if lambda > 1.0 {
        (rx * lambda.sqrt(), ry * lambda.sqrt())
    } else {
        (rx, ry)
    };

    // F.6.5.3 — center.
    let sign = if large_arc != sweep { 1.0 } else { -1.0 };
    let numer = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let denom = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let co = sign * (numer / denom).max(0.0).sqrt();
    let cxp = co * rx * y1p / ry;
    let cyp = -co * ry * x1p / rx;
    let cx = cos_phi * cxp - sin_phi * cyp + (from.x + to.x) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (from.y + to.y) / 2.0;

    // F.6.5.4 — start angle and sweep extent.
    fn angle(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 { -a } else { a }
    }
    let theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }

    // Sample; ~16 segments per full turn is plenty at icon sizes.
    let n = ((delta.abs() / (std::f32::consts::FRAC_PI_8)).ceil() as usize).max(1);
    for i in 1..=n {
        let theta = theta1 + delta * (i as f32 / n as f32);
        let ex = rx * theta.cos();
        let ey = ry * theta.sin();
        pts.push(pos2(
            cos_phi * ex - sin_phi * ey + cx,
            sin_phi * ex + cos_phi * ey + cy,
        ));
    }
}

fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2::new(x, y)
}

fn flush(pts: &mut Vec<Pos2>, out: &mut Subpaths) {
    if pts.len() >= 2 {
        out.push((std::mem::take(pts), false));
    } else {
        pts.clear();
    }
}
