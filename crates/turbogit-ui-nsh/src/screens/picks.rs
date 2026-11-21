//! The file-dialog seam. The egui app picked folders and save paths
//! synchronously through `AppState`'s `dir_picker`, `patch_writer`, and
//! `patch_reader` closures; nsh picks are async, so the screens await
//! `pick_paths` / `pick_save_path` in a task and answer the state from the
//! await's tail. `Ok(None)` is the cancel path and reports itself, exactly
//! as the egui seam's `None` did.

use std::path::PathBuf;

use nsh_core::file_dialog::{OpenDialog, SaveDialog};
use nsh_core::prelude::*;
use turbogit_app::state::{AppState, Toast};

use crate::bridge::AppHandle;

/// Asks for one folder and runs `enter` on it. A cancelled or unavailable
/// dialog leaves the state alone and queues the notice the egui seam
/// queued, so a failed pick never opens a project.
pub fn pick_folder(
    handle: AppHandle,
    purpose: &'static str,
    enter: impl FnOnce(&mut AppState, PathBuf) + 'static,
) {
    spawn(async move {
        let answer = pick_paths(OpenDialog::folders().button_label(purpose)).await;
        handle.defer(move |s| match answer {
            Ok(Some(mut picked)) if !picked.is_empty() => {
                let dir = picked.remove(picked.len() - 1);
                enter(s, dir);
            }
            Ok(None) | Ok(Some(_)) => {
                s.ui.toast = Some(Toast::error(format!("{purpose}: no folder selected")));
            }
            Err(_) => {
                s.ui.toast = Some(Toast::error(format!(
                    "{purpose}: no folder picker available"
                )));
            }
        });
    });
}

/// Asks for a save path and runs `enter` on it; the same cancel and
/// unavailable answers as [`pick_folder`].
pub fn pick_save(
    handle: AppHandle,
    purpose: &'static str,
    file_name: &'static str,
    enter: impl FnOnce(&mut AppState, PathBuf) + 'static,
) {
    spawn(async move {
        let answer = pick_save_path(SaveDialog::new().file_name(file_name).title(purpose)).await;
        handle.defer(move |s| match answer {
            Ok(Some(path)) => enter(s, path),
            Ok(None) => {
                s.ui.toast = Some(Toast::error(format!("{purpose}: no path selected")));
            }
            Err(_) => {
                s.ui.toast = Some(Toast::error(format!("{purpose}: no save dialog available")));
            }
        });
    });
}
