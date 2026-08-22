//! Persistence for TurboGit project state.
//!
//! All on-disk state lives under a per-project `.turbogit/` directory (the
//! analog of IntelliJ's `.idea/`). This layer NEVER touches `.git/`. State is
//! serialized as RON via `serde`.

use crate::error::{TgError, TgResult};
use crate::model::*;
use std::fs;
use std::path::{Path, PathBuf};

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

/// Lightweight UI-only state persisted across sessions (Epic J4): active tab,
/// recently opened repositories, and the draft commit message.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct UiPersist {
    pub tab: String,
    pub recent_repos: Vec<PathBuf>,
    pub draft_message: String,
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
