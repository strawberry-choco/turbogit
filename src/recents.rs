//! Global recents store (ADR-0005).
//!
//! Recent projects must be visible on the welcome screen before any project
//! is open, so they live in ONE global file under the OS config directory —
//! `dirs::config_dir()/TurboGit/recents.ron` — holding `{ path, name,
//! last_opened }` only. This is the app's only global state file; everything
//! else stays per-project. Branch indicators are computed live at render and
//! never persisted (a stored snapshot would go stale the moment the user
//! switches branches outside TurboGit).

use crate::error::TgResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Maximum number of recent projects kept in the store.
pub const MAX_RECENTS: usize = 10;

/// One recently-opened project row on the welcome screen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
    /// Unix timestamp (milliseconds) of the last time this project was
    /// opened. Millisecond precision keeps same-second opens ordered.
    pub last_opened: i64,
}

/// The contents of the global recents file, newest-first.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Recents {
    pub projects: Vec<RecentProject>,
}

/// Path of the global recents file inside `config_dir` (ADR-0005:
/// `<config_dir>/TurboGit/recents.ron`).
pub fn recents_file(config_dir: &Path) -> PathBuf {
    config_dir.join("TurboGit").join("recents.ron")
}

/// The production config dir (`dirs::config_dir()/TurboGit`), or `None` when
/// the OS has no config directory. Tests inject a temp dir instead.
pub fn default_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("TurboGit"))
}

/// Load the recents store. A missing or corrupt file degrades to an empty
/// store rather than failing the app.
pub fn load(config_dir: &Path) -> Recents {
    match std::fs::read_to_string(recents_file(config_dir)) {
        Ok(raw) => ron::de::from_str(&raw).unwrap_or_default(),
        Err(_) => Recents::default(),
    }
}

/// Persist the recents store as pretty RON.
pub fn save(config_dir: &Path, recents: &Recents) -> TgResult<()> {
    let file = recents_file(config_dir);
    std::fs::create_dir_all(file.parent().unwrap_or_else(|| Path::new(".")))?;
    let pretty = ron::ser::PrettyConfig::new();
    let text = ron::ser::to_string_pretty(recents, pretty)
        .map_err(|e| crate::error::TgError::Parse(format!("failed to serialize recents: {e}")))?;
    std::fs::write(file, text)?;
    Ok(())
}

/// Record `path` as just-opened: upsert by path, derive `name` from the final
/// path component, sort newest-first, cap at [`MAX_RECENTS`], then persist.
/// Returns the updated store.
pub fn record(config_dir: &Path, path: &Path) -> Recents {
    let mut recents = load(config_dir);
    record_into(&mut recents, path);
    // Best-effort persistence: a read-only config dir must not break opening.
    let _ = save(config_dir, &recents);
    recents
}

/// In-memory upsert used by [`record`] (pure, unit-testable).
pub fn record_into(recents: &mut Recents, path: &Path) {
    recents.projects.retain(|p| p.path != path);
    // Millisecond clocks can hand back the same value twice within one
    // millisecond (e.g. a rapid re-open). Bump past the newest stored entry
    // so the just-touched row always sorts above it deterministically.
    let mut now = chrono::Utc::now().timestamp_millis();
    if let Some(newest) = recents.projects.iter().map(|p| p.last_opened).max()
        && now <= newest
    {
        now = newest + 1;
    }
    recents.projects.push(RecentProject {
        path: path.to_path_buf(),
        name: project_name(path),
        last_opened: now,
    });
    recents
        .projects
        .sort_by_key(|p| std::cmp::Reverse(p.last_opened));
    recents.projects.truncate(MAX_RECENTS);
}

/// Display name for a project directory: its final component.
fn project_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Human-readable last-opened meta line ("Last opened 2026-08-22 14:03").
pub fn format_last_opened(unix_millis: i64) -> String {
    let secs = unix_millis.div_euclid(1000);
    let nanos = (unix_millis.rem_euclid(1000)) * 1_000_000;
    let t = chrono::DateTime::<chrono::Local>::from(
        chrono::DateTime::from_timestamp(secs, nanos as u32)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH),
    );
    format!("Last opened {}", t.format("%Y-%m-%d %H:%M"))
}
