use eframe::{App, Frame};
use egui::Ui;
use turbogit_app::state::AppState;
use turbogit_ui::theme;
use turbogit_ui::ui;

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
        state.patch_writer = Some(Box::new(|| {
            rfd::FileDialog::new()
                .set_title("Create patch")
                .set_file_name("patch.diff")
                .save_file()
        }));
        state.patch_reader = Some(Box::new(|| {
            rfd::FileDialog::new().set_title("Apply patch").pick_file()
        }));
        Self { state }
    }
}

impl App for TurbogitApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        let ctx = ui.ctx();

        theme::configure_style(ctx);

        // Background incoming check (issue #27): tick the scheduler, then
        // idle the frame loop until the next poll is due, egui sleeps
        // between frames, so without the wake-up a poll could only fire
        // when the user happened to move the mouse.
        if let Some(wait) = self.state.tick_incoming_poll(std::time::Instant::now()) {
            ctx.request_repaint_after(wait);
        }

        if self.state.drain_events() > 0 {
            ctx.request_repaint();
        }

        ui::render(ui, &mut self.state);
    }
}
