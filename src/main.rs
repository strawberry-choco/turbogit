//! TurboGit binary entry point. The real logic lives in the `turbogit` lib.

use eframe::NativeOptions;
use std::path::PathBuf;
use turbogit::app::TurbogitApp;

fn main() {
    // Project directory: optional first CLI arg, else the current working dir.
    let project_dir: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

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
            turbogit::theme::install_fonts(&cc.egui_ctx);
            Ok(Box::new(TurbogitApp::new(project_dir)) as Box<dyn eframe::App>)
        }),
    );
    if let Err(e) = result {
        eprintln!("TurboGit failed to start: {e}");
        std::process::exit(1);
    }
}
