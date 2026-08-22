//! eframe application entry point (eframe 0.35 `App::ui` API).
//!
//! Owns [`AppState`] and pumps the event channel every frame — the async
//! pattern from `execution-plan.md` §3.1. In 0.35 the `App` trait exposes
//! `fn ui(&mut self, ui: &mut Ui, frame)`, and `egui::Context` is reached via
//! `ui.ctx()`.

use crate::state::AppState;
use crate::ui;
use eframe::{App, Frame};
use egui::Ui;

pub struct TurbogitApp {
    pub state: AppState,
}

impl TurbogitApp {
    pub fn new(project_dir: std::path::PathBuf) -> Self {
        Self {
            state: AppState::new(project_dir),
        }
    }
}

impl App for TurbogitApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let ctx = ui.ctx();

        // Dark-only design tokens (ADR-0003); idempotent per-frame application.
        crate::theme::configure_style(ctx);

        // Drain worker-thread events and apply them to state, then repaint.
        // The pump itself lives on AppState so headless harnesses get
        // production parity (issue #13).
        if self.state.drain_events() > 0 {
            ctx.request_repaint();
        }

        ui::render(ui, &mut self.state);
    }
}
