use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::DisplayName;

pub const MAX_RECENTS: usize = 10;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecentKind {
    #[default]
    Project,
    Workspace,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
    /// Unix timestamp (milliseconds) of the last time this project was
    /// opened. Millisecond precision keeps same-second opens ordered.
    pub last_opened: i64,
    #[serde(default)]
    pub kind: RecentKind,
    #[serde(default)]
    pub repo_count: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Recents {
    pub projects: Vec<RecentProject>,
}

pub fn recents_file(config_dir: &Path) -> PathBuf {
    config_dir.join("TurboGit").join("recents.ron")
}

pub fn default_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("TurboGit"))
}

pub fn load(config_dir: &Path) -> Recents {
    match std::fs::read_to_string(recents_file(config_dir)) {
        Ok(raw) => ron::de::from_str(&raw).unwrap_or_default(),
        Err(_) => Recents::default(),
    }
}

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

pub fn record(config_dir: &Path, path: &Path) -> Recents {
    let mut recents = load(config_dir);
    record_into(&mut recents, path);
    let _ = save(config_dir, &recents);
    recents
}

pub fn record_into(recents: &mut Recents, path: &Path) {
    recents.projects.retain(|p| p.path != path);
    let mut now = chrono::Utc::now().timestamp_millis();
    if let Some(newest) = recents.projects.iter().map(|p| p.last_opened).max()
        && now <= newest
    {
        now = newest.saturating_add(1);
    }
    recents.projects.push(RecentProject {
        path: path.to_path_buf(),
        name: path.display_name_lossy_or(&path.to_string_lossy()),
        last_opened: now,
        kind: RecentKind::Project,
        repo_count: None,
    });
    recents
        .projects
        .sort_by_key(|p| std::cmp::Reverse(p.last_opened));
    recents.projects.truncate(MAX_RECENTS);
}

pub fn record_workspace(config_dir: &Path, path: &Path, repo_count: usize) -> Recents {
    let mut recents = load(config_dir);
    record_workspace_into(&mut recents, path, repo_count);
    let _ = save(config_dir, &recents);
    recents
}

pub fn record_workspace_into(recents: &mut Recents, path: &Path, repo_count: usize) {
    record_into(recents, path);
    if let Some(row) = recents.projects.iter_mut().find(|p| p.path == *path) {
        row.kind = RecentKind::Workspace;
        row.repo_count = Some(repo_count);
    }
}

pub fn remove(config_dir: &Path, path: &Path) -> Recents {
    let mut recents = load(config_dir);
    remove_into(&mut recents, path);
    let _ = save(config_dir, &recents);
    recents
}

pub fn remove_into(recents: &mut Recents, path: &Path) -> bool {
    let before = recents.projects.len();
    recents.projects.retain(|p| p.path != path);
    recents.projects.len() != before
}

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
    fn remove_into_drops_only_the_named_row_and_reports_whether_it_was_there() {
        let mut recents = Recents {
            projects: vec![
                RecentProject {
                    path: PathBuf::from("C:/projects/alpha"),
                    name: "alpha".into(),
                    last_opened: 3,
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

        recents.projects.insert(
            0,
            RecentProject {
                path: PathBuf::from("C:/projects/oldest"),
                name: "oldest".into(),
                last_opened: 1,
                kind: RecentKind::Project,
                repo_count: None,
            },
        );
        assert!(
            remove_into(&mut recents, Path::new("C:/projects/alpha")),
            "removing a stored path reports that a row was dropped"
        );
        let paths: Vec<_> = recents.projects.iter().map(|p| &p.path).collect();
        assert_eq!(
            paths,
            vec![
                &PathBuf::from("C:/projects/oldest"),
                &PathBuf::from("C:/workspaces/big"),
            ],
            "the neighbours keep their order and their kind/repo_count"
        );

        assert!(
            !remove_into(&mut recents, Path::new("C:/projects/alpha")),
            "removing an absent path is a no-op, not a false report"
        );
        assert_eq!(recents.projects.len(), 2, "the store is left as it was");
    }

    #[test]
    fn remove_persists_the_dropped_row() {
        let config = tempfile::tempdir().unwrap();
        let kept = PathBuf::from("C:/projects/kept");
        let gone = PathBuf::from("C:/projects/gone");
        record(config.path(), &kept);
        record(config.path(), &gone);
        assert_eq!(load(config.path()).projects.len(), 2);

        let after = remove(config.path(), &gone);

        assert_eq!(
            after.projects.iter().map(|p| &p.path).collect::<Vec<_>>(),
            vec![&kept]
        );
        let reloaded = load(config.path());
        assert_eq!(
            reloaded
                .projects
                .iter()
                .map(|p| &p.path)
                .collect::<Vec<_>>(),
            vec![&kept],
            "the removed row must not come back on the next launch"
        );
    }

    #[test]
    fn old_short_recents_rows_deserialize_as_project_with_no_repo_count() {
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
