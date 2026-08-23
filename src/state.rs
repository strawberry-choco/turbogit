//! Application state: owns the Git engine (the [`GitExecutor`] seam), the
//! multi-root model, canonical settings, the project directory, the event
//! channel, and all UI-only ephemeral state. The UI reads from here and never
//! calls git directly; long ops are dispatched to worker threads via
//! [`AppState::run_git`].

use crate::core::changes;
use crate::engine::{AppEvent, GitExecutor};
use crate::error::TgResult;
use crate::model::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One root's outgoing commits for the push dialog tree (issue #20).
#[derive(Clone)]
pub struct OutgoingRoot {
    pub id: RootId,
    pub name: String,
    /// Enriched commits newest-first, or the engine error string.
    pub commits: Result<Vec<Commit>, String>,
}

/// Persistent input fields for the modal dialogs (kept across redraws).
#[derive(Default)]
pub struct DialogState {
    // Push
    pub push_remote: String,
    pub push_branch: String,
    pub force_push: bool,
    /// Narrow scope to the selected root's explicit Remote/Branch target
    /// (issue #20); the default batch push covers every root (ADR-0006).
    pub push_current_branch_only: bool,
    /// Outgoing-commit tree snapshot built once when the dialog opens.
    pub push_outgoing: Option<Vec<OutgoingRoot>>,
    /// Which root node the user clicked — filters the changed-files PREVIEW
    /// only, never the batch push scope (ADR-0006).
    pub push_preview_root: Option<RootId>,
    // Merge
    pub merge_target: String,
    pub merge_no_ff: bool,
    pub merge_squash: bool,
    pub merge_no_commit: bool,
    pub merge_no_verify: bool,
    // Rebase
    pub rebase_onto: String,
    pub rebase_merges: bool,
    pub rebase_keep_empty: bool,
    pub rebase_update_refs: bool,
    pub rebase_autosquash: bool,
    // New branch
    pub new_branch_name: String,
    pub new_branch_start: String,
    pub new_branch_checkout: bool,
    // Tag
    pub tag_name: String,
    pub tag_msg: String,
    pub tag_push: bool,
    // Shelve / Stash
    pub shelve_name: String,
    pub stash_msg: String,
    pub stash_keep: bool,
    // Interactive rebase plan (built on open)
    pub rebase_plan: Option<Vec<crate::model::RebasePlanEntry>>,
    pub rebase_base: Option<String>,
}

/// Which central tab is active.
///
/// Settings left the strip in issue #16 (spec §9.1 correction): it is a
/// gear-only modal now and can never be the active tool window.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    #[default]
    Commit,
    Log,
}

/// Which Commit-window sub-tab is active (issue #18).
///
/// Local Changes / Unversioned Files carry active data; Shelf / Stash render
/// labeled placeholder panes until their Phase-J features land (ADR-0008).
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitSubTab {
    /// Tracked modifications + merge conflicts (the classic commit surface).
    #[default]
    LocalChanges,
    /// Untracked files, includable in commits.
    UnversionedFiles,
    /// IDE-managed patch store — Phase J placeholder.
    Shelf,
    /// Git-native stash — Phase J placeholder.
    Stash,
}

/// A modal dialog currently open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialog {
    Push,
    Merge,
    Rebase,
    InteractiveRebase,
    NewBranch,
    Tag,
    Shelve,
    Stash,
}

/// A destructive action awaiting explicit confirmation (Epic C8 / Epic H3).
#[derive(Clone)]
pub enum PendingConfirm {
    Discard { changes: Vec<Change> },
    DeleteLocalBranch { name: String },
    DeleteRemoteBranch { remote: String, name: String },
    InitHere,
    CloneRepo,
}

/// What the diff viewer should display.
#[derive(Clone)]
pub struct DiffTarget {
    pub root: RootId,
    pub left: Option<String>,
    pub right: Option<String>,
    pub path: Option<PathBuf>,
}

/// Which working-tree comparison the diff viewer shows (issue #13, spec §8.4
/// revision chips). Each variant maps to one documented git pair:
///
/// | Chip    | Pair               | Engine call            |
/// |---------|--------------------|------------------------|
/// | `Repo`  | HEAD ↔ worktree    | `git diff HEAD`        |
/// | `Staged`| HEAD ↔ index       | `git diff --cached`    |
/// | `Local` | index ↔ worktree   | `git diff`             |
///
/// Only used when the viewer renders a working-tree comparison; explicit
/// commit-to-commit targets (Git Log) keep their fixed revision pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DiffComparison {
    /// HEAD ↔ worktree.
    Repo,
    /// HEAD ↔ index.
    Staged,
    /// index ↔ worktree.
    #[default]
    Local,
}

/// UI-only ephemeral state (never persisted).
#[derive(Default)]
pub struct UiState {
    /// Persistent inputs for the modal dialogs.
    pub dlg: DialogState,
    // Commit tab
    pub commit_message: String,
    pub amend: bool,
    pub selected: HashSet<PathBuf>,
    pub recent_messages: Vec<String>,
    /// Active sub-tab inside the Commit tool window (issue #18).
    pub commit_subtab: CommitSubTab,
    // tabs / popups
    pub tab: Tab,
    pub branches_popup: bool,
    pub branch_filter: String,
    pub log_filter: String,
    // Git Log four-pane workspace (issue #12)
    /// Live search text for the branches pane.
    pub log_branch_filter: String,
    /// Roots filter: `None` shows every root's commits, `Some(id)` narrows.
    pub log_root_filter: Option<RootId>,
    /// File selected in the changed-files pane.
    pub log_selected_file: Option<PathBuf>,
    /// Active path scope in Git Log (issue #19): `Some(path)` narrows the
    /// graph to only the commits touching that path (set from the
    /// changed-files pane's "Show history for file..." context menu).
    pub log_path_scope: Option<PathBuf>,
    pub selected_commit: Option<CommitId>,
    pub diff: Option<DiffTarget>,
    pub dialog: Option<Dialog>,
    pub vcs_popup: bool,
    pub settings_open: bool,
    /// Draft copy of the loaded [`VcsSettings`] while the Settings modal is
    /// open (issue #16). Edited in place, compared against
    /// [`AppState::settings`] for dirty-gating; Reset restores it from the
    /// loaded values and Cancel/close drops it without persisting.
    pub settings_draft: Option<VcsSettings>,
    // 3-way merge editor (Epic E6)
    pub conflict_open: Option<PathBuf>,
    pub conflict_segs: Vec<(String, String, bool)>, // (text, other_text, is_conflict)
    pub conflict_res: Vec<u8>,                      // 0=ours, 1=theirs, 2=both per conflict
    pub conflict_text: String,                      // editable composed result
    pub shelves: Vec<Shelf>,
    // commit-tab inline diff preview (Epic C3)
    pub preview_change: Option<PathBuf>,
    // diff viewer (Epic E: async cache + layout)
    pub diff_cache: Option<(String, String)>,
    pub diff_loading: bool,
    pub diff_error: Option<String>,
    pub diff_side_by_side: bool,
    pub diff_current_hunk: usize,
    // diff viewer working-tree comparison chips + whitespace toggle (issue #13)
    pub diff_comparison: DiffComparison,
    pub diff_ignore_whitespace: bool,
    // command palette (Epic F5)
    pub command_palette: bool,
    pub command_query: String,
    // transient
    pub toast: Option<String>,
    pub toast_shown_at: Option<f64>,
    pub busy: bool,
    // confirmation-gated destructive actions (Epic C8)
    pub confirm: Option<crate::state::PendingConfirm>,
    // recently opened repositories (Epic J4)
    pub recent_repos: Vec<PathBuf>,
    // Branches popup (issue #14): recently checked-out branches (≤5, newest first).
    pub recent_branches: Vec<String>,
    /// Keyboard cursor into the branches popup's flattened selectable rows
    /// (issue #14: ↑/↓ move it, Enter checks the highlighted row out).
    pub branches_cursor: usize,
    // IDE shell visibility model (issue #9, spec §9.2): true → the central
    // body routes to the Welcome page instead of the active tool window.
    // Derived true whenever no root is open (`AppState::show_welcome`).
    pub welcome_visible: bool,
    // Welcome page (issue #10): in-memory copy of the global recents store
    // (ADR-0005), loaded at launch and refreshed on every recorded open.
    pub recent_projects: Vec<crate::recents::RecentProject>,
    /// In-memory branch-indicator cache for visible recents: path →
    /// `(branch, computed_at)`. Computed live at render, never persisted.
    pub welcome_branch_cache: HashMap<PathBuf, (Option<String>, std::time::Instant)>,
    // Inline clone form on the Welcome page (issue #10).
    pub welcome_clone_url: String,
    pub welcome_shallow: bool,
    /// Set by the Clone action card; focuses the URL input on the next render.
    pub welcome_focus_clone: bool,
    /// User-toggleable shell regions (View menu); not persisted in v1.
    pub show_toolbar: bool,
    pub show_status_bar: bool,
}

pub struct AppState {
    pub project_dir: PathBuf,
    /// The Git engine. This interface is the seam (ADR-0001).
    pub executor: Arc<dyn GitExecutor>,
    /// Canonical engine settings (git binary path, update method, …).
    pub settings: VcsSettings,
    pub multi: MultiRootManager,
    pub tx: Sender<AppEvent>,
    pub rx: Receiver<AppEvent>,
    pub selected_root: Option<RootId>,
    pub clone_url: String,
    pub last_error: Option<String>,
    pub ui: UiState,
    /// Cached commit logs keyed by root (refreshed on demand / after ops).
    pub log_cache: HashMap<RootId, Vec<Commit>>,
    /// Cached ref decorations keyed by root, then commit id (issue #12).
    pub ref_cache: HashMap<RootId, HashMap<CommitId, Vec<CommitRef>>>,
    /// Cached changed-file lists keyed by (root, commit id) (issue #12).
    pub files_cache: HashMap<(RootId, CommitId), Vec<Change>>,
    /// Cached path-scoped logs keyed by (root, scoped path) (issue #19).
    pub log_path_cache: HashMap<(RootId, PathBuf), Vec<Commit>>,
    /// Ahead/behind of each root's current branch vs its upstream (Epic D3).
    pub ahead_behind: HashMap<RootId, (usize, usize)>,
    /// Override for the OS config dir hosting the global recents file
    /// (ADR-0005). `None` → `recents::default_config_dir()`. Tests inject a
    /// temp dir so the real user configuration is never touched.
    pub recents_config_dir: Option<PathBuf>,
    /// Native folder-picker seam for the Welcome Open/Initialize flows.
    /// Production wires `rfd`; tests inject closures returning fixed paths.
    pub dir_picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
}

impl AppState {
    pub fn new(project_dir: PathBuf) -> Self {
        Self::launch(Some(project_dir))
    }

    /// Launch with an explicit project directory: roots are discovered and
    /// the shell is entered directly — `turbogit <path>` (ADR-0004). `None`
    /// lands on the Welcome screen without scanning any directory.
    pub fn launch(project_dir: Option<PathBuf>) -> Self {
        Self::launch_in(project_dir, None)
    }

    /// Launch flow (ADR-0004): with a project directory the shell opens
    /// straight away; without one the Welcome screen is the landing surface
    /// and no CWD scan happens. `recents_config_dir` overrides the OS config
    /// dir hosting the global recents file (tests inject a temp dir).
    pub fn launch_in(project_dir: Option<PathBuf>, recents_config_dir: Option<PathBuf>) -> Self {
        let (tx, rx) = unbounded();
        let settings = VcsSettings::default();
        let executor: Arc<dyn GitExecutor> = Arc::new(crate::engine::cli::CliExecutor {
            settings: settings.clone(),
        });
        let mut state = Self {
            project_dir: project_dir.clone().unwrap_or_default(),
            executor,
            settings,
            multi: MultiRootManager::default(),
            tx,
            rx,
            selected_root: None,
            clone_url: String::new(),
            last_error: None,
            // Shell regions start visible; View-menu toggles flip these.
            ui: UiState {
                show_toolbar: true,
                show_status_bar: true,
                ..UiState::default()
            },
            log_cache: HashMap::new(),
            ref_cache: HashMap::new(),
            files_cache: HashMap::new(),
            log_path_cache: HashMap::new(),
            ahead_behind: HashMap::new(),
            recents_config_dir,
            dir_picker: None,
        };

        // Global recents (ADR-0005) load before anything is open so the
        // Welcome screen can list them.
        if let Some(cfg) = state.recents_config() {
            state.ui.recent_projects = crate::recents::load(&cfg).projects;
        }

        match project_dir {
            Some(dir) => {
                state.project_dir = dir;
                state.rescan();

                // Restore persisted UI state (Epic J4): active tab, draft message, recent repos.
                let ui = crate::persistence::load_ui_state(&state.project_dir);
                state.ui.tab = match ui.tab.as_str() {
                    "Log" => Tab::Log,
                    // Legacy persisted "History" (removed in issue #19)
                    // gracefully falls back to the Commit tab.
                    _ => Tab::Commit,
                };
                state.ui.commit_message = ui.draft_message;
                state.ui.recent_repos = ui.recent_repos;
            }
            None => {
                // No project directory supplied: land on Welcome (ADR-0004).
                state.ui.welcome_visible = true;
            }
        }
        state
    }

    /// The effective config dir for the global recents file (ADR-0005).
    fn recents_config(&self) -> Option<PathBuf> {
        self.recents_config_dir
            .clone()
            .or_else(crate::recents::default_config_dir)
    }

    /// Persist lightweight UI state (active tab, draft message, recent repos).
    pub fn persist_ui(&self) {
        let ui = crate::persistence::UiPersist {
            tab: match self.ui.tab {
                Tab::Log => "Log",
                _ => "Commit",
            }
            .to_string(),
            recent_repos: self.ui.recent_repos.clone(),
            draft_message: self.ui.commit_message.clone(),
        };
        let _ = crate::persistence::save_ui_state(&self.project_dir, &ui);
    }

    /// Re-discover roots under `project_dir`, register any new ones, and
    /// dispatch a fresh asynchronous status scan for every registered root.
    pub fn rescan(&mut self) {
        let paths =
            crate::core::multi_root::discover_roots(self.executor.as_ref(), &self.project_dir);
        let results =
            crate::core::multi_root::register_all(self.executor.as_ref(), &mut self.multi, &paths);
        for r in &results {
            if let Err(e) = r {
                self.last_error = Some(e.to_string());
            }
        }
        if self.selected_root.is_none() {
            self.selected_root = self.multi.roots.first().map(|r| r.id.clone());
        }

        let executor = self.executor.clone();
        let tx = self.tx.clone();
        for root in &self.multi.roots {
            let root_path = root.id.0.clone();
            let exec = executor.clone();
            let tx_status = tx.clone();
            std::thread::spawn(move || {
                let res = exec.status(&root_path);
                let _ = tx_status.send(AppEvent::StatusScanned {
                    root: RootId(root_path),
                    status: res,
                });
            });
            // Ahead/behind of the current branch vs its upstream (Epic D3).
            let exec2 = executor.clone();
            let tx2 = tx.clone();
            let rp = root.id.0.clone();
            std::thread::spawn(move || {
                let ab = (|| -> TgResult<(usize, usize)> {
                    let branches = exec2.branches(&rp)?;
                    let cur = exec2.current_branch(&rp)?;
                    if let Some(b) = branches
                        .iter()
                        .find(|b| b.kind == BranchKind::Local && cur.as_deref() == Some(&b.name))
                    {
                        if let Some(up) = &b.tracking {
                            return exec2.ahead_behind(&rp, b.name.as_str(), up);
                        }
                    }
                    Ok((0, 0))
                })();
                if let Ok((ahead, behind)) = ab {
                    let _ = tx2.send(AppEvent::AheadBehind {
                        root: RootId(rp),
                        ahead,
                        behind,
                    });
                }
            });
        }
    }

    /// Fetch (and cache) the commit log for a root on a worker thread.
    pub fn fetch_log(&mut self, root: RootId) {
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = executor.log(&root.0, &LogOpts::default());
            let _ = tx.send(AppEvent::LogLoaded { root, commits: res });
        });
    }

    /// Dispatch a git operation on a worker thread. `work` receives the
    /// engine (`GitExecutor`) and returns a `TgResult<()>`; the result is posted as an
    /// `OpCompleted` event and the UI re-scans on completion.
    pub fn run_git<W>(&mut self, label: String, work: W)
    where
        W: FnOnce(&dyn GitExecutor) -> TgResult<()> + Send + 'static,
    {
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        self.ui.busy = true;
        std::thread::spawn(move || {
            let res = work(executor.as_ref());
            let _ = tx.send(AppEvent::OpCompleted { label, result: res });
        });
    }

    /// Execute a confirmed destructive action (Epic C8). The UI gates these
    /// behind a confirmation dialog and only calls this on explicit OK.
    pub fn run_confirmed(&mut self, c: PendingConfirm) {
        match c {
            PendingConfirm::Discard { changes } => {
                let root = self.selected_path();
                self.run_git("Discard changes".into(), move |v| {
                    if let Some(r) = &root {
                        changes::discard_changes(v, r, &changes)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeleteLocalBranch { name } => {
                let root = self.selected_path();
                self.run_git(format!("Delete branch {name}"), move |v| {
                    if let Some(r) = &root {
                        v.branch_delete(r, &name, false)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeleteRemoteBranch { remote, name } => {
                let root = self.selected_path();
                self.run_git("Delete remote branch".into(), move |v| {
                    if let Some(r) = &root {
                        v.branch_delete_remote(r, &remote, &name)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::InitHere => self.init_repo(),
            PendingConfirm::CloneRepo => self.clone_repo(),
        }
    }

    /// `git init` at the project dir, persist the mapping, then rescan.
    pub fn init_repo(&mut self) {
        if let Err(e) = self.executor.init(&self.project_dir) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(&self.project_dir, &self.project_dir, Vcs::Git);
        self.rescan();
    }

    /// Open `dir` as the active project (issue #10): retarget the project
    /// directory, rediscover roots from scratch, enter the shell, and record
    /// the project in the global recents store (ADR-0005).
    pub fn open_project(&mut self, dir: &Path) {
        self.project_dir = dir.to_path_buf();
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.log_cache.clear();
        self.ahead_behind.clear();
        self.rescan();
        self.ui.welcome_visible = false;
        self.record_recent(dir);
    }

    /// Create a real repository at `dir` through the engine seam and enter
    /// it (Welcome "Initialize Repository" card, issue #10).
    pub fn initialize_and_enter(&mut self, dir: &Path) {
        if let Err(e) = self.executor.init(dir) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(dir, dir, Vcs::Git);
        self.open_project(dir);
    }

    /// Close every open project and return to the Welcome screen
    /// (File → Welcome, ADR-0004).
    pub fn close_all_projects(&mut self) {
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.log_cache.clear();
        self.ahead_behind.clear();
        self.ui.welcome_visible = true;
    }

    /// Upsert `dir` into the global recents store and refresh the in-memory
    /// copy used by the Welcome page.
    pub fn record_recent(&mut self, dir: &Path) {
        if let Some(cfg) = self.recents_config() {
            let recents = crate::recents::record(&cfg, dir);
            self.ui.recent_projects = recents.projects;
        }
    }

    /// Drop the cached branch indicators so the next render recomputes them
    /// live (ADR-0005: never stored, always fresh at render time).
    pub fn invalidate_welcome_branches(&mut self) {
        self.ui.welcome_branch_cache.clear();
    }

    /// Clone `clone_url` into a sibling directory, then rescan.
    pub fn clone_repo(&mut self) {
        let url = self.clone_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let name = url
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .trim_end_matches(".git")
            .to_string();
        let dest = self.project_dir.join(&name);
        if let Err(e) = GitExecutor::clone(&*self.executor, &url, &dest, None) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(&self.project_dir, &dest, Vcs::Git);
        self.rescan();
    }

    /// Drain worker-thread events and apply them to state. Production calls
    /// this every frame from `app.rs`; headless harnesses call it for
    /// production parity (issue #13: async diff tests).
    pub fn drain_events(&mut self) -> usize {
        let mut drained = 0usize;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::StatusScanned { root, status } => {
                    if let Some(r) = self.multi.roots.iter_mut().find(|r| r.id == root) {
                        match status {
                            Ok(s) => r.status = s,
                            Err(e) => self.last_error = Some(e.to_string()),
                        }
                    }
                }
                AppEvent::LogLoaded { root, commits } => match commits {
                    Ok(c) => {
                        self.log_cache.insert(root, c);
                    }
                    Err(e) => self.last_error = Some(e.to_string()),
                },
                AppEvent::OpCompleted { label, result } => {
                    self.ui.busy = false;
                    match result {
                        Ok(()) => {
                            self.ui.toast = Some(format!("✓ {label}"));
                            // Refresh roots (status/branches/remotes) + log.
                            self.rescan();
                            if let Some(id) = &self.selected_root {
                                let id = id.clone();
                                self.fetch_log(id);
                            }
                        }
                        Err(e) => {
                            self.ui.toast = Some(format!("✗ {label}: {e}"));
                            self.last_error = Some(e.to_string());
                        }
                    }
                }
                AppEvent::Error(msg) => {
                    self.ui.busy = false;
                    self.last_error = Some(msg);
                }
                AppEvent::DiffReady { key, result } => {
                    self.ui.diff_loading = false;
                    match result {
                        Ok(text) => {
                            self.ui.diff_error = None;
                            self.ui.diff_cache = Some((key, text));
                        }
                        Err(e) => {
                            self.ui.diff_error = Some(e.to_string());
                            if self
                                .ui
                                .diff_cache
                                .as_ref()
                                .map(|(k, _)| k != &key)
                                .unwrap_or(false)
                            {
                                self.ui.diff_cache = None;
                            }
                        }
                    }
                }
                AppEvent::AheadBehind {
                    root,
                    ahead,
                    behind,
                } => {
                    self.ahead_behind.insert(root, (ahead, behind));
                }
                _ => {}
            }
            drained += 1;
        }
        drained
    }

    /// The currently selected root's path (or None).
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_root.as_ref().map(|r| r.0.clone())
    }

    /// Welcome-vs-shell routing (issue #9, spec §9.2): the central body shows
    /// the Welcome page when no repository root is open, or when the user
    /// explicitly returned to it (File → Welcome). A project opened at launch
    /// (`turbogit <path>`) enters the shell directly.
    pub fn show_welcome(&self) -> bool {
        self.multi.roots.is_empty() || self.ui.welcome_visible
    }
}
