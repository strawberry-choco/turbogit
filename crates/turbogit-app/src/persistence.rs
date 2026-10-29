use std::fs;
use std::path::{Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;

use crate::state::Tab;

pub fn turbogit_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".turbogit")
}

pub fn state_path(project_dir: &Path) -> PathBuf {
    turbogit_dir(project_dir).join("state.ron")
}

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

pub fn save(project_dir: &Path, state: &ProjectState) -> TgResult<()> {
    fs::create_dir_all(turbogit_dir(project_dir))?;

    let pretty = ron::ser::PrettyConfig::new();
    let text = ron::ser::to_string_pretty(state, pretty)
        .map_err(|e| TgError::Parse(format!("failed to serialize state: {e}")))?;
    fs::write(state_path(project_dir), text)?;
    Ok(())
}

pub fn load_settings(project_dir: &Path) -> TgResult<VcsSettings> {
    let state = load_or_default(project_dir)?;
    Ok(state.settings)
}

pub fn save_settings(project_dir: &Path, settings: &VcsSettings) -> TgResult<()> {
    let mut state = load_or_default(project_dir)?;
    state.settings = settings.clone();
    save(project_dir, &state)
}

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

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct BranchesViewOptions {
    pub group_by_directory: bool,
    pub group_by_repository: bool,
    pub show_recent: bool,
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

macro_rules! durable_ui_state {
    (
        $(
            $(#[$attr:meta])*
            $name:ident : $ty:ty => $(| $restore:ident)? $($write_only:ident)? ;
        )*
    ) => {
        #[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
        pub struct UiPersist {
            $( $(#[$attr])* pub $name: $ty, )*
        }

        impl UiPersist {
            #[allow(clippy::clone_on_copy)]
            pub fn capture(ui: &crate::state::UiState) -> Self {
                Self {
                    $(
                        $( $name: ui.$restore.clone(), )?
                        $( $name: ui.$write_only.clone(), )?
                    )*
                }
            }

            pub fn restore_into(self, ui: &mut crate::state::UiState) {
                $(
                    $( ui.$restore = self.$name; )?
                )*
            }
        }
    };
}

mod tab_spelling {
    use crate::state::Tab;
    use serde::{Deserialize, Deserializer, Serializer};

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
    #[serde(with = "tab_spelling")]
    tab: Tab => | tab;

    recent_repos: Vec<PathBuf> => | recent_repos;

    draft_message: String => commit_message;

    #[serde(default)]
    smart_group_rules: Vec<crate::smart_rules::SmartGroupRule> => | smart_group_rules;

    #[serde(default)]
    pinned_views: Vec<crate::pinned_views::PinnedView> => | pinned_views;

    #[serde(default)]
    bulk_history: Vec<crate::bulk_history::BulkRunRecord> => | bulk_history;

    #[serde(default)]
    recent_custom_commands: Vec<String> => | recent_custom_commands;

    #[serde(default)]
    sidebar_collapsed: std::collections::HashSet<String> => | sidebar_collapsed;

    #[serde(default)]
    branches_view: BranchesViewOptions => | branches_view;

    #[serde(default)]
    diff_policies: turbogit_domain::model::DiffPolicies => | diff_policies;
}

fn ui_path(project_dir: &Path) -> PathBuf {
    turbogit_dir(project_dir).join("ui.ron")
}

pub fn load_ui_state(project_dir: &Path) -> UiPersist {
    let path = ui_path(project_dir);
    match fs::read_to_string(&path) {
        Ok(raw) => ron::de::from_str(&raw).unwrap_or_default(),
        Err(_) => UiPersist::default(),
    }
}

pub fn save_ui_state(project_dir: &Path, ui: &UiPersist) -> TgResult<()> {
    fs::create_dir_all(turbogit_dir(project_dir))?;
    let pretty = ron::ser::PrettyConfig::new();
    let text = ron::ser::to_string_pretty(ui, pretty)
        .map_err(|e| TgError::Parse(format!("failed to serialize ui state: {e}")))?;
    fs::write(ui_path(project_dir), text)?;
    Ok(())
}
