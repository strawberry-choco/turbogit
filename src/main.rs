#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> std::process::ExitCode {
    let project_dir: Option<PathBuf> = std::env::args().nth(1).map(PathBuf::from);
    turbogit_ui::launch::launch(project_dir)
}
