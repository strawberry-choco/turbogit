//! The desktop launch: the native backend at the shell's launch size, dark,
//! with the crate's `theme.json` as the tokens.

#![cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]

use std::path::PathBuf;

use nsh_core::desktop::MainWindowOptions;
use nsh_core::geometry::{px, size};

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

/// Runs the app on the native backend. `Some(dir)` opens that project;
/// `None` lands on Welcome. `TURBOGIT_CONFIG` points the recents store at a
/// scratch directory.
pub fn launch(project_dir: Option<PathBuf>) -> std::process::ExitCode {
    env_logger::init();
    let config = std::env::var_os("TURBOGIT_CONFIG").map(PathBuf::from);
    let root = platform::Root::new::<crate::AppRoot>(move || crate::AppRootProps {
        handle: crate::bridge::AppHandle::new(turbogit_app::state::AppState::launch_in(
            project_dir.clone(),
            config.clone(),
        )),
    })
    .tokens(crate::tokens())
    .appearance(platform::Appearance::Dark)
    .main_window({
        let mut options = MainWindowOptions::default();
        options.attributes.min_size = Some(Some(size(px(1000.0), px(680.0))));
        options
    });
    platform::run_app(
        platform::AppSource::Rust(root),
        platform::AppOptions {
            title: "TurboGit".to_owned(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            app_id: "dev.turbogit.turbogit".to_owned(),
            size: (1280, 820),
            ..Default::default()
        },
    )
}
