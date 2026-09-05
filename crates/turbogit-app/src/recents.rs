//! Global recents store (ADR-0005).
//!
//! Recent projects must be visible on the welcome screen before any project
//! is open, so they live in ONE global file under the OS config directory —
//! `dirs::config_dir()/TurboGit/recents.ron` — holding `{ path, name,
//! last_opened }` only. This is the app's only global state file; everything
//! else stays per-project. Branch indicators are computed live at render and
//! never persisted (a stored snapshot would go stale the moment the user
//! switches branches outside TurboGit).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;

/// Maximum number of recent projects kept in the store.
pub const MAX_RECENTS: usize = 10;

/// Kind of a recent entry: a single-project repository ([`RecentKind::Project`])
/// or an attached multi-repo workspace root ([`RecentKind::Workspace`], issue
/// #34). Workspace rows carry a [`RecentProject::repo_count`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecentKind {
    /// A single repository opened directly (clone / open / init).
    #[default]
    Project,
    /// A workspace root whose subtree was deep-scanned and registered.
    Workspace,
}

/// One recently-opened project row on the welcome screen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
    /// Unix timestamp (milliseconds) of the last time this project was
    /// opened. Millisecond precision keeps same-second opens ordered.
    pub last_opened: i64,
    /// Whether this row is a workspace root. `#[serde(default)]` keeps
    /// pre-#34 recents.ron rows loading as single-repo projects.
    #[serde(default)]
    pub kind: RecentKind,
    /// Repos registered under a workspace root, `None` for single-project
    /// rows. `#[serde(default)]` degrades legacy rows gracefully.
    #[serde(default)]
    pub repo_count: Option<usize>,
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
    let text = ron::ser::to_string_pretty(recents, pretty).map_err(|e| {
        turbogit_domain::error::TgError::Parse(format!("failed to serialize recents: {e}"))
    })?;
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
        // `last_opened` is parsed unvalidated from recents.ron, so `newest`
        // can be i64::MAX; saturate instead of overflowing the bump.
        now = newest.saturating_add(1);
    }
    recents.projects.push(RecentProject {
        path: path.to_path_buf(),
        name: project_name(path),
        last_opened: now,
        kind: RecentKind::Project,
        repo_count: None,
    });
    recents
        .projects
        .sort_by_key(|p| std::cmp::Reverse(p.last_opened));
    recents.projects.truncate(MAX_RECENTS);
}

/// Record `path` as an attached workspace root (issue #34): same upsert /
/// sort / cap policy as [`record`], but the row is marked
/// [`RecentKind::Workspace`] and carries the number of repos indexed under it.
pub fn record_workspace(config_dir: &Path, path: &Path, repo_count: usize) -> Recents {
    let mut recents = load(config_dir);
    record_workspace_into(&mut recents, path, repo_count);
    // Best-effort persistence, mirroring [`record`].
    let _ = save(config_dir, &recents);
    recents
}

/// In-memory upsert for a workspace root (pure, unit-testable). Buffers on
/// [`record_into`] then marks the row as a workspace with its repo count.
pub fn record_workspace_into(recents: &mut Recents, path: &Path, repo_count: usize) {
    record_into(recents, path);
    if let Some(row) = recents.projects.iter_mut().find(|p| p.path == *path) {
        row.kind = RecentKind::Workspace;
        row.repo_count = Some(repo_count);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `last_opened` is loaded unvalidated from recents.ron; an i64::MAX
    /// entry must not overflow the same-millisecond bump (`newest + 1`).
    #[test]
    fn record_into_saturates_at_i64_max_last_opened() {
        let mut recents = Recents {
            projects: vec![RecentProject {
                path: PathBuf::from("C:/projects/saturated"),
                name: "saturated".into(),
                last_opened: i64::MAX,
                kind: RecentKind::Project,
                repo_count: None,
            }],
        };
        record_into(&mut recents, Path::new("C:/projects/fresh"));
        // The bump saturated (both entries tie at i64::MAX), so the stable
        // sort keeps the pre-existing row first — assert by path, not index.
        let fresh = recents
            .projects
            .iter()
            .find(|p| p.path == Path::new("C:/projects/fresh"))
            .expect("fresh entry recorded");
        assert_eq!(
            fresh.last_opened,
            i64::MAX,
            "the bump must saturate, not overflow"
        );
    }

    #[test]
    fn recents_schema_roundtrips_project_and_workspace_entries() {
        let config = tempfile::tempdir().unwrap();
        let recents = Recents {
            projects: vec![
                RecentProject {
                    path: PathBuf::from("C:/projects/alpha"),
                    name: "alpha".into(),
                    last_opened: 1,
                    kind: RecentKind::Project,
                    repo_count: None,
                },
                RecentProject {
                    path: PathBuf::from("C:/workspaces/big"),
                    name: "big".into(),
                    last_opened: 2,
                    kind: RecentKind::Workspace,
                    repo_count: Some(41),
                },
            ],
        };
        save(config.path(), &recents).unwrap();
        let loaded = load(config.path());
        assert_eq!(loaded.projects, recents.projects);
        assert_eq!(loaded.projects[0].kind, RecentKind::Project);
        assert_eq!(loaded.projects[1].kind, RecentKind::Workspace);
        assert_eq!(loaded.projects[1].repo_count, Some(41));
    }

    #[test]
    fn old_short_recents_rows_deserialize_as_project_with_no_repo_count() {
        // A pre-#34 recents.ron has neither `kind` nor `repo_count`; loading
        // it must degrade to a single-repo Project (backward compatible).
        let config = tempfile::tempdir().unwrap();
        let file = recents_file(config.path());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            "(\n    projects: [\n        (\n            path: \"C:/projects/alpha\",\n            name: \"alpha\",\n            last_opened: 1755000000000,\n        ),\n    ],\n)\n",
        )
        .unwrap();

        let loaded = load(config.path());
        assert_eq!(loaded.projects.len(), 1);
        let row = &loaded.projects[0];
        assert_eq!(row.name, "alpha");
        assert_eq!(row.kind, RecentKind::Project);
        assert_eq!(row.repo_count, None);
    }
}
