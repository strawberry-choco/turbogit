//! Runs the application directly on the machine's native backend, dark, at
//! the shell's launch size, with the crate's `theme.json` as the tokens.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
use nsh_platform_linux as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "windows")]
use nsh_platform_windows as platform;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn main() -> impl std::process::Termination {
    env_logger::init();
    let root = platform::Root::of::<turbogit_ui::App>()
        .tokens(turbogit_ui::tokens())
        .appearance(platform::Appearance::Dark);
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

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!(
    "Run this binary on macOS, Windows or Linux; use the TSX entry with nsh run for iOS and Android."
);
