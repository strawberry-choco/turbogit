//! TurboGit binary entry point — the workspace's composition root.

use eframe::NativeOptions;
use std::path::PathBuf;
use turbogit::app::TurbogitApp;
use turbogit_ui::theme;

fn main() {
    // Project directory: optional first CLI arg (ADR-0004). With a path the
    // shell opens straight away; without one the Welcome screen is shown —
    // no implicit CWD scan.
    let project_dir: Option<PathBuf> = std::env::args().nth(1).map(PathBuf::from);

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::vec2(1280.0, 820.0))
            .with_min_inner_size(egui::vec2(1000.0, 680.0))
            .with_resizable(true)
            .with_title("TurboGit"),
        multisampling: 4,
        ..Default::default()
    };
    let result = eframe::run_native(
        "TurboGit",
        options,
        Box::new(|cc| {
            // Embedded JetBrains Mono (ADR-0002), once before the first frame.
            theme::install_fonts(&cc.egui_ctx);
            Ok(Box::new(TurbogitApp::launch(project_dir)) as Box<dyn eframe::App>)
        }),
    );
    if let Err(e) = result {
        eprintln!("TurboGit failed to start: {e}");
        std::process::exit(1);
    }
}
