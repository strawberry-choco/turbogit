//! Application state: owns the Git engine (the [`GitExecutor`] seam), the
//! multi-root model, canonical settings, the project directory, the event
//! channel, and all UI-only ephemeral state. The UI reads from here and never
//! calls git directly; long ops are dispatched to worker threads via
//! [`AppState::run_git`].
use crate::events::AppEvent;
use crate::granular;
use crate::root_caches::{Affected, RootCaches};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine::build_executor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::changes;

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
    /// Verbatim `git push --dry-run` output captured by the Preview button
    /// (issue #21): `Ok(report)` when git accepted the push, `Err(git
    /// stderr)` when it rejected it. Rendered as-is, never paraphrased.
    pub push_preview_output: Option<Result<String, String>>,
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
    pub rebase_plan: Option<Vec<turbogit_domain::model::RebasePlanEntry>>,
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

/// Semantic category of a transient feedback message (issue #22). Drives the
/// toast's icon and STATE_* color; replaces the old ✓/✗ string-prefix
/// sniffing with a typed kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    /// Operation succeeded (`STATE_SUCCESS`, check icon).
    Success,
    /// Attention needed but nothing failed (`STATE_WARNING`).
    Warning,
    /// Operation failed (`STATE_ERROR`).
    Error,
    /// Neutral information (`STATE_INFO`).
    Info,
}

/// One transient feedback message with its semantic kind (issue #22).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
}

impl Toast {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Success,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Warning,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Error,
            message: message.into(),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Info,
            message: message.into(),
        }
    }
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
    /// Inline filter over the changed-file list (spec R7, CONTEXT.md "File
    /// filter"): shared by both active Commit sub-tabs, matched
    /// case-insensitively against file paths. Persists across root switches
    /// and refreshes within the session; Esc while focused or manual edits
    /// clear it.
    pub file_filter: String,
    /// Set by `/` or the Filter Files palette action; the Commit window
    /// focuses the filter input on the next render and clears the flag.
    pub focus_file_filter: bool,
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
    // 3-way merge editor (Epic E6; redesigned in issue #15)
    pub conflict_open: Option<PathBuf>,
    pub conflict_segs: Vec<(String, String, bool)>, // (text, other_text, is_conflict)
    /// Per-conflict resolution: `None` = unresolved, `Some(0)` = ours,
    /// `Some(1)` = theirs, `Some(2)` = both ("Ignore").
    pub conflict_res: Vec<Option<u8>>,
    /// Read-only composed result shown in the editor's Result pane
    /// (free-text editing is explicitly deferred).
    pub conflict_text: String,
    pub shelves: Vec<Shelf>,
    // commit-tab inline diff preview (Epic C3)
    pub preview_change: Option<PathBuf>,
    // diff viewer (Epic E: async cache + layout)
    pub diff_cache: Option<(String, String)>,
    pub diff_loading: bool,
    pub diff_error: Option<String>,
    pub diff_side_by_side: bool,
    /// Non-text diff pane results (decoded image bytes + binary sizes, spec
    /// R8) keyed by load key — the plain-data cache lives in
    /// [`crate::diff_data`], not the UI module. Bounded few entries, evicts
    /// oldest; invalidated wholesale with root refreshes like `diff_cache`
    /// (CONTEXT.md "Root caches" philosophy). GPU textures are a UI-layer
    /// concern: they are keyed by [`Self::pane_generation`], never stored
    /// here.
    pub pane_bytes: crate::diff_data::PaneCache,
    /// Generation of [`Self::pane_bytes`]: bumped on every wholesale clear so
    /// the UI layer can drop its lazily-uploaded GPU textures without the
    /// application state naming any egui type (DDD split issue 04).
    pub pane_generation: u64,
    /// Load key currently being fetched on a worker thread — one in-flight
    /// non-text pane load at a time (mirrors `diff_loading`).
    pub pane_bytes_loading: Option<String>,
    /// The single hunk of the open diff that all hunk navigation and
    /// granular verbs act on (CONTEXT.md "Current hunk"): buttons, hover,
    /// and keyboard navigation set it; stage/unstage consume it. Reset to
    /// the first hunk whenever a fresh diff load starts.
    pub diff_current_hunk: usize,
    // diff viewer working-tree comparison chips + whitespace toggle (issue #13)
    pub diff_comparison: DiffComparison,
    pub diff_ignore_whitespace: bool,
    /// Armed edge press for F7/Shift+F7 cross-file navigation (spec R7): the
    /// direction and instant of the last edge nudge. A same-direction repeat
    /// inside [`crate::diff_data::EDGE_WINDOW`] crosses to the adjacent
    /// changed file; anything else re-arms.
    pub hunk_nav_armed_edge: Option<(crate::diff_data::Dir, std::time::Instant)>,
    /// The preview path a granular stage/unstage was last dispatched for
    /// (spec R2 story 9). Consumed by the op-completion handler to decide
    /// whether the file just left the changelist.
    pub pending_granular: Option<PathBuf>,
    /// Paths whose last granular op completed with nothing unstaged left
    /// (spec R2 story 9): they stop being listed in the changelist buckets.
    /// Ephemeral by design — entries drop as soon as the path regains
    /// unstaged changes or disappears from status entirely.
    pub granularly_completed: HashSet<PathBuf>,
    /// Sub-hunk line selections (spec R2 story 3): preview path → hunk index
    /// → ordinals over that hunk's +/- lines (`HunkSelection::Lines`
    /// semantics). Ephemeral: cleared whenever the path's diff cache entry
    /// changes — which also covers every successful granular op, since ops
    /// invalidate the cache and force a reload.
    pub line_selections: HashMap<PathBuf, BTreeMap<usize, BTreeSet<usize>>>,
    // command palette (Epic F5)
    pub command_palette: bool,
    pub command_query: String,
    // transient
    /// Last feedback message with its semantic kind (issue #22).
    pub toast: Option<Toast>,
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
    /// The root-keyed cache layer (logs, ref decorations, changed files,
    /// path-scoped logs, ahead/behind) behind one interface (CONTEXT.md
    /// "Root caches").
    pub caches: RootCaches,
    /// Override for the OS config dir hosting the global recents file
    /// (ADR-0005). `None` → `recents::default_config_dir()`. Tests inject a
    /// temp dir so the real user configuration is never touched.
    pub recents_config_dir: Option<PathBuf>,
    /// Native folder-picker seam for the Welcome Open/Initialize flows.
    /// Production wires `rfd`; tests inject closures returning fixed paths.
    pub dir_picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    /// Headless-harness mode: completed ops refresh root status synchronously
    /// instead of spawning background rescans (see `for_roots`).
    sync_refresh: bool,
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
        let executor = build_executor(&settings);
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
            caches: RootCaches::default(),
            recents_config_dir,
            dir_picker: None,
            sync_refresh: false,
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

    /// Headless-harness constructor (see CONTEXT.md "Headless harness"): a
    /// deterministic [`AppState`] over explicit repository roots.
    ///
    /// Roots are registered synchronously through the same registration path
    /// production uses ([`turbogit_services::multi_root::register_all`]); no background
    /// threads are spawned, and completed operations refresh root status
    /// synchronously instead of rescanning on workers. Panics if any root cannot
    /// be snapshotted — a broken test fixture should fail at construction.
    pub fn for_roots(project_dir: &Path, roots: &[PathBuf]) -> Self {
        let (tx, rx) = unbounded();
        let settings = VcsSettings::default();
        let executor = build_executor(&settings);
        let mut state = Self {
            project_dir: project_dir.to_path_buf(),
            executor,
            settings,
            multi: MultiRootManager::default(),
            tx,
            rx,
            selected_root: None,
            clone_url: String::new(),
            last_error: None,
            // Bare UiState defaults (toolbar/status bar hidden), matching what
            // the headless suites assert against — NOT launch_in's visible shell.
            ui: UiState::default(),
            caches: RootCaches::default(),
            recents_config_dir: None,
            dir_picker: None,
            sync_refresh: true,
        };
        let results = turbogit_services::multi_root::register_all(
            state.executor.as_ref(),
            &mut state.multi,
            roots,
        );
        for r in results {
            if let Err(e) = r {
                panic!("for_roots: failed to snapshot root: {e}");
            }
        }
        state.selected_root = state.multi.roots.first().map(|r| r.id.clone());
        state
    }

    /// Override the Git engine (e.g. a recording test double).
    pub fn with_executor(mut self, executor: Arc<dyn GitExecutor>) -> Self {
        self.executor = executor;
        self
    }

    /// Override canonical engine settings (e.g. protected-branch patterns).
    pub fn with_settings(mut self, settings: VcsSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Rebuild the engine from the current settings behind the seam
    /// (ADR-0001: a changed git binary or backend applies live). The
    /// composition-root factory lives in `turbogit-engine`, so the app
    /// crate owns this call and the UI crate reaches it through here
    /// instead of depending on the adapters.
    pub fn rebuild_executor(&mut self) {
        self.executor = build_executor(&self.settings);
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
        let paths = turbogit_services::multi_root::discover_roots(
            self.executor.as_ref(),
            &self.project_dir,
        );
        let results = turbogit_services::multi_root::register_all(
            self.executor.as_ref(),
            &mut self.multi,
            &paths,
        );
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
                if let Ok((ahead, behind)) = current_branch_ahead_behind(exec2.as_ref(), &rp) {
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

    /// The one refresh seam for completed operations and manual refresh
    /// (root-caches deepening, decision 7): drop the affected roots' cache
    /// entries, refresh those roots' snapshots + ahead/behind — no root
    /// DISCOVERY here (clone/init/open have their own paths) — and refetch
    /// the selected root's log iff it is in scope.
    ///
    /// Snapshot refresh goes through [`turbogit_services::multi_root::register_all`],
    /// which replaces the registered snapshot per id (branches / HEAD /
    /// status) without scanning for new roots — what kept branch indicators
    /// fresh after checkouts pre-refactor, now scoped to the affected roots.
    ///
    /// Production computes ahead/behind on worker threads; the headless
    /// harness (`sync_refresh`) mirrors the same steps synchronously.
    pub fn refresh(&mut self, affected: Affected) {
        self.caches.invalidate(&affected);
        // The diff viewer caches raw patch text outside the root caches;
        // a completed op may have changed exactly what it shows (spec R2
        // story 8), so drop it and let the viewer reload asynchronously.
        self.ui.diff_cache = None;
        // Non-text pane bytes follow the same wholesale rule (spec R8):
        // dropped with root refreshes, never poked per field. Bumping the
        // generation tells the UI layer to drop its GPU textures too — the
        // plain-data cache never holds an egui type itself (DDD split
        // issue 04).
        self.ui.pane_bytes.clear();
        self.ui.pane_generation += 1;
        self.ui.pane_bytes_loading = None;
        // Granular exclusions follow a refresh-scoped lifetime rule owned by
        // the granular module: they only hold while the path is still fully
        // staged.
        granular::prune_on_refresh(self);
        // Only roots that are actually registered take part in the refresh.
        // The ids' shared `Arc<Path>` handles clone by refcount here and flow
        // into `register_all` / the ahead-behind refresh unchanged.
        let paths: Vec<std::sync::Arc<Path>> = match &affected {
            Affected::All => self.multi.roots.iter().map(|r| r.id.0.clone()).collect(),
            Affected::Root(id) => self
                .multi
                .roots
                .iter()
                .filter(|r| &r.id == id)
                .map(|r| r.id.0.clone())
                .collect(),
        };
        let results = turbogit_services::multi_root::register_all(
            self.executor.as_ref(),
            &mut self.multi,
            &paths,
        );
        for r in &results {
            if let Err(e) = r {
                self.last_error = Some(e.to_string());
            }
        }

        // Ahead/behind of each affected root's current branch vs upstream.
        if self.sync_refresh {
            // Headless harness: refresh synchronously, no threads.
            let executor = self.executor.clone();
            for path in paths {
                if let Ok(ab) = current_branch_ahead_behind(executor.as_ref(), &path) {
                    self.caches.store_ahead_behind(RootId(path), ab);
                }
            }
        } else {
            let executor = self.executor.clone();
            let tx = self.tx.clone();
            for rp in paths {
                let exec2 = executor.clone();
                let tx2 = tx.clone();
                std::thread::spawn(move || {
                    if let Ok((ahead, behind)) = current_branch_ahead_behind(exec2.as_ref(), &rp) {
                        let _ = tx2.send(AppEvent::AheadBehind {
                            root: RootId(rp),
                            ahead,
                            behind,
                        });
                    }
                });
            }
        }

        // Refetch the selected root's log iff it is inside the scope.
        if let Some(sel) = self.selected_root.clone() {
            let in_scope = match &affected {
                Affected::All => true,
                Affected::Root(id) => *id == sel,
            };
            if in_scope {
                if self.sync_refresh {
                    if let Ok(commits) = self.executor.log(&sel.0, &LogOpts::default()) {
                        self.caches.store_log(sel, commits);
                    }
                } else {
                    self.fetch_log(sel);
                }
            }
        }
    }

    /// Dispatch a git operation on a worker thread. `work` receives the
    /// engine (`GitExecutor`) and returns a `TgResult<()>`; the result is posted as an
    /// `OpCompleted` event and the affected roots' caches and status are
    /// refreshed on completion ([`AppState::refresh`]). Every call site
    /// declares its scope via `affected`.
    pub fn run_git<W>(&mut self, label: String, affected: Affected, work: W)
    where
        W: FnOnce(&dyn GitExecutor) -> TgResult<()> + Send + 'static,
    {
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        self.ui.busy = true;
        std::thread::spawn(move || {
            let res = work(executor.as_ref());
            let _ = tx.send(AppEvent::OpCompleted {
                label,
                affected,
                result: res,
            });
        });
    }

    /// Execute a confirmed destructive action (Epic C8). The UI gates these
    /// behind a confirmation dialog and only calls this on explicit OK.
    pub fn run_confirmed(&mut self, c: PendingConfirm) {
        match c {
            PendingConfirm::Discard { changes } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git("Discard changes".into(), affected, move |v| {
                    if let Some(r) = &root {
                        changes::discard_changes(v, r, &changes)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeleteLocalBranch { name } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git(format!("Delete branch {name}"), affected, move |v| {
                    if let Some(r) = &root {
                        v.branch_delete(r, &name, false)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeleteRemoteBranch { remote, name } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git("Delete remote branch".into(), affected, move |v| {
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
        // Drop every cache entry: the old project's roots must not leak into
        // the new one (bug fix — only logs/ahead-behind were cleared before).
        self.caches.invalidate_all();
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
        self.caches.invalidate_all();
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
                    Ok(c) => self.caches.store_log(root, c),
                    Err(e) => self.last_error = Some(e.to_string()),
                },
                AppEvent::OpCompleted {
                    label,
                    affected,
                    result,
                } => {
                    self.ui.busy = false;
                    match result {
                        Ok(()) => {
                            self.ui.toast = Some(Toast::success(label));
                            self.refresh(affected);
                            granular::settle(self);
                        }
                        Err(e) => {
                            self.ui.toast = Some(Toast::error(format!("{label}: {e}")));
                            self.last_error = Some(e.to_string());
                            granular::on_op_failed(self);
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
                AppEvent::FileBytesReady { key, old, new } => {
                    // The in-flight slot frees for the next wanted pane even
                    // when this result is already stale.
                    if self.ui.pane_bytes_loading.as_deref() == Some(key.as_str()) {
                        self.ui.pane_bytes_loading = None;
                    }
                    self.ui.pane_bytes.store(
                        key,
                        crate::diff_data::PaneEntry {
                            old: old.map(crate::diff_data::PaneSide::from_blob),
                            new: new.map(crate::diff_data::PaneSide::from_blob),
                        },
                    );
                }
                AppEvent::AheadBehind {
                    root,
                    ahead,
                    behind,
                } => {
                    self.caches.store_ahead_behind(root, (ahead, behind));
                }
                _ => {}
            }
            drained += 1;
        }
        drained
    }

    /// The currently selected root's path (or None).
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_root.as_ref().map(|r| r.0.to_path_buf())
    }

    /// Welcome-vs-shell routing (issue #9, spec §9.2): the central body shows
    /// the Welcome page when no repository root is open, or when the user
    /// explicitly returned to it (File → Welcome). A project opened at launch
    /// (`turbogit <path>`) enters the shell directly.
    pub fn show_welcome(&self) -> bool {
        self.multi.roots.is_empty() || self.ui.welcome_visible
    }
}

/// Ahead/behind of `root`'s current branch vs its upstream (Epic D3);
/// `(0, 0)` when no local branch with an upstream is checked out. Shared by
/// the asynchronous rescan and the synchronous refresh paths so both fill
/// the cache identically.
fn current_branch_ahead_behind(exec: &dyn GitExecutor, root: &Path) -> TgResult<(usize, usize)> {
    let branches = exec.branches(root)?;
    let cur = exec.current_branch(root)?;
    if let Some(b) = branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && cur.as_deref() == Some(&b.name))
        && let Some(up) = &b.tracking
    {
        return exec.ahead_behind(root, b.name.as_str(), up);
    }
    Ok((0, 0))
}
