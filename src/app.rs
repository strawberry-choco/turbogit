//! eframe application entry point (eframe 0.35 `App::ui` API).
//!
//! Owns [`AppState`] and pumps the event channel every frame — the async
//! pattern from `execution-plan.md` §3.1. In 0.35 the `App` trait exposes
//! `fn ui(&mut self, ui: &mut Ui, frame)`, and `egui::Context` is reached via
//! `ui.ctx()`.

use crate::engine::AppEvent;
use crate::state::AppState;
use crate::ui;
use eframe::{App, Frame};
use egui::Ui;

pub struct TurbogitApp {
    pub state: AppState,
}

impl TurbogitApp {
    pub fn new(project_dir: std::path::PathBuf) -> Self {
        Self::launch(Some(project_dir))
    }

    /// Launch flow (ADR-0004): `Some(dir)` enters the shell directly;
    /// `None` lands on the Welcome screen. Wires the native folder-picker
    /// seam used by the Welcome Open/Initialize/Clone flows.
    pub fn launch(project_dir: Option<std::path::PathBuf>) -> Self {
        let mut state = AppState::launch(project_dir);
        state.dir_picker = Some(Box::new(|| rfd::FileDialog::new().pick_folder()));
        Self { state }
    }
}

impl App for TurbogitApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let ctx = ui.ctx();

        // Dark-only design tokens (ADR-0003); idempotent per-frame application.
        crate::theme::configure_style(ctx);

        // Drain worker-thread events and apply them to state, then repaint.
        while let Ok(ev) = self.state.rx.try_recv() {
            match ev {
                AppEvent::StatusScanned { root, status } => {
                    if let Some(r) = self.state.multi.roots.iter_mut().find(|r| r.id == root) {
                        match status {
                            Ok(s) => r.status = s,
                            Err(e) => self.state.last_error = Some(e.to_string()),
                        }
                    }
                }
                AppEvent::LogLoaded { root, commits } => match commits {
                    Ok(c) => {
                        self.state.log_cache.insert(root, c);
                    }
                    Err(e) => self.state.last_error = Some(e.to_string()),
                },
                AppEvent::OpCompleted { label, result } => {
                    self.state.ui.busy = false;
                    match result {
                        Ok(()) => {
                            self.state.ui.toast = Some(format!("✓ {label}"));
                            // Refresh roots (status/branches/remotes) + log.
                            self.state.rescan();
                            if let Some(id) = &self.state.selected_root {
                                let id = id.clone();
                                self.state.fetch_log(id);
                            }
                        }
                        Err(e) => {
                            self.state.ui.toast = Some(format!("✗ {label}: {e}"));
                            self.state.last_error = Some(e.to_string());
                        }
                    }
                }
                AppEvent::Error(msg) => {
                    self.state.ui.busy = false;
                    self.state.last_error = Some(msg);
                }
                AppEvent::DiffReady { key, result } => {
                    self.state.ui.diff_loading = false;
                    match result {
                        Ok(text) => {
                            self.state.ui.diff_error = None;
                            self.state.ui.diff_cache = Some((key, text));
                        }
                        Err(e) => {
                            self.state.ui.diff_error = Some(e.to_string());
                            if self
                                .state
                                .ui
                                .diff_cache
                                .as_ref()
                                .map(|(k, _)| k != &key)
                                .unwrap_or(false)
                            {
                                self.state.ui.diff_cache = None;
                            }
                        }
                    }
                }
                AppEvent::AheadBehind {
                    root,
                    ahead,
                    behind,
                } => {
                    self.state.ahead_behind.insert(root, (ahead, behind));
                }
                _ => {}
            }
            ctx.request_repaint();
        }

        ui::render(ui, &mut self.state);
    }
}
