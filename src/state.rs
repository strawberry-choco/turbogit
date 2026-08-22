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
use std::path::PathBuf;
use std::sync::Arc;

/// Persistent input fields for the modal dialogs (kept across redraws).
#[derive(Default)]
pub struct DialogState {
    // Push
    pub push_remote: String,
    pub push_branch: String,
    pub force_push: bool,
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
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    #[default]
    Commit,
    Log,
    History,
    Settings,
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
    pub history_path: String,
    pub selected_commit: Option<CommitId>,
    pub diff: Option<DiffTarget>,
    pub dialog: Option<Dialog>,
    pub vcs_popup: bool,
    pub settings_open: bool,
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
    // IDE shell visibility model (issue #9, spec §9.2): true → the central
    // body routes to the Welcome page instead of the active tool window.
    // Derived true whenever no root is open (`AppState::show_welcome`).
    pub welcome_visible: bool,
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
    /// Ahead/behind of each root's current branch vs its upstream (Epic D3).
    pub ahead_behind: HashMap<RootId, (usize, usize)>,
}

impl AppState {
    pub fn new(project_dir: PathBuf) -> Self {
        let (tx, rx) = unbounded();
        let settings = VcsSettings::default();
        let executor: Arc<dyn GitExecutor> = Arc::new(crate::engine::cli::CliExecutor {
            settings: settings.clone(),
        });
        let mut state = Self {
            project_dir,
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
            ahead_behind: HashMap::new(),
        };
        state.rescan();

        // Restore persisted UI state (Epic J4): active tab, draft message, recent repos.
        let ui = crate::persistence::load_ui_state(&state.project_dir);
        state.ui.tab = match ui.tab.as_str() {
            "Log" => Tab::Log,
            "History" => Tab::History,
            _ => Tab::Commit,
        };
        state.ui.commit_message = ui.draft_message;
        state.ui.recent_repos = ui.recent_repos;
        state
    }

    /// Persist lightweight UI state (active tab, draft message, recent repos).
    pub fn persist_ui(&self) {
        let ui = crate::persistence::UiPersist {
            tab: match self.ui.tab {
                Tab::Log => "Log",
                Tab::History => "History",
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
