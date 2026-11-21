//! The selector vocabulary UI tests drive: one place spells every test id.
//! Rows key by root and commit, files by root and path, dialogs by name, so
//! a selector names what it addresses instead of which row happens to be at
//! an index.

/// A commit row of `root`, keyed by its commit id.
pub fn commit_row(root: &str, commit: &str) -> String {
    format!("row:{root}:{commit}")
}

/// A file row of `root`, keyed by its path.
pub fn file_row(root: &str, path: &str) -> String {
    format!("file:{root}:{path}")
}

/// The dialog named `name` (its title's slug: `rename-branch`).
pub fn dialog(name: &str) -> String {
    format!("dialog:{name}")
}

/// The primary action of the dialog named `name`.
pub fn dialog_action(name: &str) -> String {
    format!("dialog:{name}:action")
}

/// The tab named `name` (Commit, Log, Branches, ...).
pub fn tab(name: &str) -> String {
    format!("tab:{name}")
}

/// A toolbar control named `name`.
pub fn toolbar(name: &str) -> String {
    format!("toolbar:{name}")
}

/// The route registered under `id` (`changes`, `branches`), the placeholder
/// empty state a later phase replaces.
pub fn route(id: impl std::fmt::Display) -> String {
    format!("route:{id}")
}

/// A welcome-surface control named `name` (`clone`, `recent:alpha`).
pub fn welcome(name: impl std::fmt::Display) -> String {
    format!("welcome:{name}")
}

/// A log-surface control named `name` (`filter-branch`, `ref:main`).
pub fn log(name: impl std::fmt::Display) -> String {
    format!("log:{name}")
}

/// A workspace-picker control named `name` (`switch`, `row:alpha`).
pub fn workspace(name: impl std::fmt::Display) -> String {
    format!("workspace:{name}")
}

/// A changes-surface control named `name` (`filter`, `subtab:Shelf`).
pub fn changes(name: impl std::fmt::Display) -> String {
    format!("changes:{name}")
}

/// A changed-file row of `root`, keyed by its repository-relative path.
pub fn change_row(root: &str, path: &str) -> String {
    format!("changes:row:{root}:{path}")
}

/// The changed-file row's staging checkbox.
pub fn change_check(root: &str, path: &str) -> String {
    format!("changes:check:{root}:{path}")
}

/// A diff-viewer control named `name` (`next`, `add:12`, `check:12`).
pub fn diff(name: impl std::fmt::Display) -> String {
    format!("diff:{name}")
}

/// An image or binary pane control named `name`.
pub fn image(name: impl std::fmt::Display) -> String {
    format!("image:{name}")
}

/// A shelf or stash row of `root`, keyed by its name or index.
pub fn shelf(name: impl std::fmt::Display) -> String {
    format!("shelf:{name}")
}

/// A commit-card control named `name` (`message`, `action`, `amend`).
pub fn commit(name: impl std::fmt::Display) -> String {
    format!("commit:{name}")
}

/// The banner strip.
pub fn banner() -> String {
    "banner".to_string()
}

/// The shell's status bar.
pub fn status_bar() -> String {
    "status-bar".to_string()
}

/// The status bar's busy indicator.
pub fn status_bar_busy() -> String {
    "status-bar:busy".to_string()
}

/// The shell's activity feed, named `name` (`feed`, `feed-clear`).
pub fn activity(name: impl std::fmt::Display) -> String {
    format!("activity:{name}")
}

/// The gallery section titled `name`.
pub fn gallery_section(name: &str) -> String {
    format!("gallery:{name}")
}

/// A gallery demo control named `name`, one per control state.
pub fn demo(name: &str) -> String {
    format!("demo:{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_key_by_root_and_their_key() {
        assert_eq!(commit_row("main", "abc123"), "row:main:abc123");
        assert_eq!(file_row("main", "src/lib.rs"), "file:main:src/lib.rs");
        assert_ne!(
            commit_row("main", "abc123"),
            commit_row("other", "abc123"),
            "the same commit id in two roots addresses two rows"
        );
    }

    #[test]
    fn dialogs_key_by_name() {
        assert_eq!(dialog("rename-branch"), "dialog:rename-branch");
        assert_eq!(
            dialog_action("rename-branch"),
            "dialog:rename-branch:action"
        );
    }
}
