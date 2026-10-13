//! Persistence for TurboGit project state.
//!
//! All on-disk state lives under a per-project `.turbogit/` directory (the
//! analog of IntelliJ's `.idea/`). This layer NEVER touches `.git/`. State is
//! serialized as RON via `serde`.
//!
//! `.turbogit/ui.ron`'s layout — which of the 138 [`UiState`] fields are
//! durable, in what order they are written, and how the active tab is spelled
//! — belongs to the one `durable_ui_state!` list below. Its ten rows are
//! written on every save and nine of them are read back: the tenth,
//! `draft_message`, is written by every trigger and restored by none, which is
//! a declared row in that list rather than an accident, and restoring it is a
//! product ruling rather than a refactor.

use std::fs;
use std::path::{Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;

use crate::state::Tab;

/// Returns the path to the project's `.turbogit/` metadata directory.
pub fn turbogit_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".turbogit")
}

/// Returns the path to the serialized `state.ron` file.
pub fn state_path(project_dir: &Path) -> PathBuf {
    turbogit_dir(project_dir).join("state.ron")
}

/// Load the project state, or a default if none is persisted yet.
///
/// Ensures `.turbogit/` exists. If `state.ron` is present it is deserialized;
/// a parse failure is surfaced as [`TgError::Parse`] rather than silently
/// swallowed. A missing file yields [`ProjectState::default`].
pub fn load_or_default(project_dir: &Path) -> TgResult<ProjectState> {
    fs::create_dir_all(turbogit_dir(project_dir))?;

    let path = state_path(project_dir);
    if !path.exists() {
        return Ok(ProjectState::default());
    }

    let raw = fs::read_to_string(&path)?;
    let state = ron::de::from_str::<ProjectState>(&raw)
        .map_err(|e| TgError::Parse(format!("failed to parse {}: {e}", path.display())))?;
    Ok(state)
}

/// Serialize the project state to `.turbogit/state.ron`.
pub fn save(project_dir: &Path, state: &ProjectState) -> TgResult<()> {
    fs::create_dir_all(turbogit_dir(project_dir))?;

    let pretty = ron::ser::PrettyConfig::new();
    let text = ron::ser::to_string_pretty(state, pretty)
        .map_err(|e| TgError::Parse(format!("failed to serialize state: {e}")))?;
    fs::write(state_path(project_dir), text)?;
    Ok(())
}

/// Load just the [`VcsSettings`] from the project state.
pub fn load_settings(project_dir: &Path) -> TgResult<VcsSettings> {
    let state = load_or_default(project_dir)?;
    Ok(state.settings)
}

/// Replace and persist the project's [`VcsSettings`].
pub fn save_settings(project_dir: &Path, settings: &VcsSettings) -> TgResult<()> {
    let mut state = load_or_default(project_dir)?;
    state.settings = settings.clone();
    save(project_dir, &state)
}

/// Register a directory → VCS mapping, skipping duplicates for the same dir.
pub fn add_mapping(project_dir: &Path, dir: &Path, vcs: Vcs) -> TgResult<()> {
    let mut state = load_or_default(project_dir)?;
    if !state.mappings.iter().any(|m| m.directory == dir) {
        state.mappings.push(DirMapping {
            directory: dir.to_path_buf(),
            vcs,
        });
    }
    save(project_dir, &state)
}

/// The Branches surface's four view toggles, behind its settings gear.
///
/// These are **grouping and visibility**, never data: nothing here changes what
/// git is asked or what an action does, only how the same branches are arranged
/// and how much of the arrangement is shown. They are the popup's only
/// persisted state on either side of the parity comparison,
/// which is why they live with the other durable UI
/// state rather than in the tree's ephemeral expand/collapse block —
/// [`crate::state::BranchesGroups`] answers "is this group open right now",
/// this answers "does this group exist for me at all".
///
/// All four default to **on**, which is the shape the surface had before any of
/// them were togglable: directory grouping, repository grouping, the Recent
/// group and the Tags group were each unconditional.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct BranchesViewOptions {
    /// Group branches by their `/` prefix, showing each leaf without it.
    pub group_by_directory: bool,
    /// Group branches under a band per repository.
    pub group_by_repository: bool,
    /// Show the Recent group at all.
    pub show_recent: bool,
    /// Show the Tags group at all.
    pub show_tags: bool,
}

impl Default for BranchesViewOptions {
    fn default() -> Self {
        Self {
            group_by_directory: true,
            group_by_repository: true,
            show_recent: true,
            show_tags: true,
        }
    }
}

/// The durable UI set — every [`UiState`](crate::state::UiState) field that
/// survives a restart — declared once, with three products: the on-disk
/// struct, the direction [`UiPersist::capture`] runs and the save entry runs,
/// and the direction [`UiPersist::restore_into`] runs at launch. A row added
/// here is written and read back by construction; there is no third
/// hand-synchronized list left to forget.
///
/// Read a row as *"`UiPersist` field of this type, captured from the named
/// `UiState` field, and — when the name carries a `|` — restored into it too."*
/// A row without the marker is captured and written but never restored, and
/// that is a declared decision rather than a missing line: `draft_message` is
/// the only one, and its row says why.
///
/// `serde` attributes stay **per row**. `tab`, `recent_repos` and
/// `draft_message` carry none, so a `ui.ron` missing `tab` still fails to
/// parse and [`load_ui_state`] still answers a wholly default struct; a
/// blanket `default` would instead load the other nine, which is a behaviour
/// change and not a simplification.
macro_rules! durable_ui_state {
    (
        $(
            $(#[$attr:meta])*
            $name:ident : $ty:ty => $(| $restore:ident)? $($write_only:ident)? ;
        )*
    ) => {
        /// Lightweight UI-only state persisted across sessions (Epic J4),
        /// emitted from [`durable_ui_state!`] — that list is where what is
        /// durable is declared.
        #[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
        pub struct UiPersist {
            $( $(#[$attr])* pub $name: $ty, )*
        }

        impl UiPersist {
            /// The save direction: every durable row cloned out of a live
            /// [`UiState`](crate::state::UiState). One uniform expression for
            /// all ten rows — every durable type is already `Clone`, the two
            /// `Copy` ones included, so no row needs a capture mode of its own.
            // `clone` on the two `Copy` rows is the uniform expression, not
            // an oversight: a per-row copy mode would be a second thing to
            // keep in step with the list.
            #[allow(clippy::clone_on_copy)]
            pub fn capture(ui: &crate::state::UiState) -> Self {
                Self {
                    $(
                        $( $name: ui.$restore.clone(), )?
                        $( $name: ui.$write_only.clone(), )?
                    )*
                }
            }

            /// The restore direction: move each restoring row into the
            /// [`UiState`](crate::state::UiState) field it names, and touch
            /// nothing else.
            ///
            /// Deliberately partial, and deliberately not
            /// `From<UiPersist> for UiState`: the other 128 `UiState` fields
            /// are session state the two constructors already disagree about,
            /// so a whole-struct conversion would quietly take a side.
            pub fn restore_into(self, ui: &mut crate::state::UiState) {
                $(
                    $( ui.$restore = self.$name; )?
                )*
            }
        }
    };
}

/// The active tab's spelling in `.turbogit/ui.ron`, **both** directions in one
/// table: the save side cannot name a tab the load side does not know, because
/// both read this list.
///
/// The strings are the file's own spelling, not the strip's current labels —
/// the Commit tab has always written `"Commit"` while the strip calls it
/// "Changes" — so nothing here may be tidied towards the widget.
mod tab_spelling {
    use crate::state::Tab;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Tab ↔ on-disk label, one row per tab. This is the whole vocabulary of
    /// `ui.ron`'s `tab` key in both directions.
    const LABELS: [(Tab, &str); 5] = [
        (Tab::Commit, "Commit"),
        (Tab::Log, "Log"),
        (Tab::Branches, "Branches"),
        (Tab::Worktrees, "Worktrees"),
        (Tab::Submodules, "Submodules"),
    ];

    fn label(tab: Tab) -> &'static str {
        match LABELS.iter().find(|(t, _)| *t == tab) {
            Some((_, label)) => label,
            None => "Commit",
        }
    }

    fn tab_of(label: &str) -> Tab {
        // Legacy persisted "History" (removed in issue #19)
        // gracefully falls back to the Commit tab.
        // Its own arm though the catch-all below answers the same: a file from
        // a build that HAD the tab is a courtesy, and should not read as a typo.
        if label == "History" {
            return Tab::Commit;
        }
        match LABELS.iter().find(|(_, l)| *l == label) {
            Some((tab, _)) => *tab,
            None => Tab::Commit,
        }
    }

    pub(crate) fn serialize<S: Serializer>(tab: &Tab, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(label(*tab))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Tab, D::Error> {
        Ok(tab_of(&String::deserialize(d)?))
    }
}

durable_ui_state! {
    /// The active tab, spelled on disk as the label string — `tab: "Log"`,
    /// never a variant name — so a file written by any build reads back the
    /// same and the legacy `"History"` spelling lands on the Commit tab
    /// rather than on an empty pane. See [`tab_spelling`].
    #[serde(with = "tab_spelling")]
    tab: Tab => | tab;

    /// Recently opened repository roots (see `crate::recents` for the
    /// *global* recents list, which is a different thing and a different
    /// file — ADR-0005).
    recent_repos: Vec<PathBuf> => | recent_repos;

    /// The commit draft — **written to `.turbogit/ui.ron` by every trigger
    /// and read back by none.**
    ///
    /// The row reaches disk from every incidental save, not only from a
    /// commit: a sidebar folder collapsed, settings applied or a bulk run
    /// settled all write the whole projection with the draft in it, and no
    /// route restores it, so a draft does not survive a restart. The missing
    /// `|` *is* that asymmetry — it sat as a missing line ninety lines below
    /// the line that writes it, which is why it survived until this
    /// declaration existed. Restoring the draft is user-perceivable and a
    /// product ruling, so the row stays write-only.
    draft_message: String => commit_message;

    /// User-defined smart group rules (issue #07). `serde(default)` keeps
    /// files written before rules existed loading cleanly.
    #[serde(default)]
    smart_group_rules: Vec<crate::smart_rules::SmartGroupRule> => | smart_group_rules;

    /// Pinned multi-repo selections (issue #08). `serde(default)` keeps
    /// files written before views existed loading cleanly.
    #[serde(default)]
    pinned_views: Vec<crate::pinned_views::PinnedView> => | pinned_views;

    /// Recent bulk operations history (issue #12). `serde(default)` keeps
    /// files written before the history existed loading cleanly.
    #[serde(default)]
    bulk_history: Vec<crate::bulk_history::BulkRunRecord> => | bulk_history;

    /// Recent custom commands (issue 13), newest first, capped.
    /// `serde(default)` keeps files written before the list existed loading
    /// cleanly.
    #[serde(default)]
    recent_custom_commands: Vec<String> => | recent_custom_commands;

    /// Sidebar PROJECTS collapse state, keyed by **relative path**
    /// (sidebar-project-tree issue 03): one entry per collapsed folder or
    /// repo-with-children. Stale name-keyed entries from older sessions
    /// match nothing and are harmless. `serde(default)` keeps files written
    /// before the key existed loading cleanly.
    #[serde(default)]
    sidebar_collapsed: std::collections::HashSet<String> => | sidebar_collapsed;

    /// The Branches surface's four toggles, behind its settings gear.
    /// `serde(default)` keeps files written before the gear existed loading
    /// cleanly, and the default is the shape the surface had before any of
    /// them were togglable.
    #[serde(default)]
    branches_view: BranchesViewOptions => | branches_view;

    /// The diff viewer's two view policies. Like the branch view's toggles these
    /// are **presentation, never data**: they change which rows the viewer counts
    /// as different and how a difference is highlighted, not what git is asked or
    /// what staging applies. Two developers reviewing one commit can only agree on
    /// what counts as a change if the agreement survives a restart.
    /// `serde(default)` keeps files written before the key existed loading cleanly.
    #[serde(default)]
    diff_policies: turbogit_domain::model::DiffPolicies => | diff_policies;
}

fn ui_path(project_dir: &Path) -> PathBuf {
    turbogit_dir(project_dir).join("ui.ron")
}

/// Load UI state, returning a default if absent or unparseable.
pub fn load_ui_state(project_dir: &Path) -> UiPersist {
    let path = ui_path(project_dir);
    match fs::read_to_string(&path) {
        Ok(raw) => ron::de::from_str(&raw).unwrap_or_default(),
        Err(_) => UiPersist::default(),
    }
}

/// Persist UI state to `.turbogit/ui.ron`.
pub fn save_ui_state(project_dir: &Path, ui: &UiPersist) -> TgResult<()> {
    fs::create_dir_all(turbogit_dir(project_dir))?;
    let pretty = ron::ser::PrettyConfig::new();
    let text = ron::ser::to_string_pretty(ui, pretty)
        .map_err(|e| TgError::Parse(format!("failed to serialize ui state: {e}")))?;
    fs::write(ui_path(project_dir), text)?;
    Ok(())
}
