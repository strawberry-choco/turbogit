use std::path::{Path, PathBuf};

use chrono::Local;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{
    DisplayName, Patch, PatchCreatedPolicy, PatchEncoding, PatchOrigin, RootId,
};
use turbogit_services::patch_create;

use crate::activity::{ActivityEntry, ActivityKind};
use crate::root_caches::Affected;
use crate::state::{AppState, Dialog, PatchCreated, PatchSource, Toast};

#[derive(Default)]
pub(crate) struct Memos {
    raw_key: Option<(PatchOrigin, bool)>,
    limit_key: Option<(PatchOrigin, bool, String)>,
}

pub fn open(state: &mut AppState, origin: PatchOrigin) {
    let Some(root) = state.selected_root.clone() else {
        state.show_toast(Toast::info(
            "Create patch needs a repository: open a workspace first.",
        ));
        return;
    };
    open_on(state, root, origin);
}

pub fn open_on(state: &mut AppState, root: RootId, origin: PatchOrigin) {
    let needs_comment = origin.needs_comment();
    reset(state, root.clone(), origin.clone());
    if needs_comment {
        state.ui.dlg.create_patch_comment = state.ui.commit_message.clone();
        state.ui.dialog = Some(Dialog::CreatePatchComment);
        return;
    }
    let subject = origin_subject(state, &root);
    state.ui.dlg.create_patch_comment = subject.clone();
    state.ui.dlg.create_patch_name = patch_create::file_name(&subject);
    open_settings(state);
}

pub fn open_from_selection(state: &mut AppState) {
    let origin = local_changes_origin(state);
    open(state, origin);
}

pub fn copy_from_selection(state: &mut AppState) {
    let origin = local_changes_origin(state);
    copy_to_clipboard(state, origin);
}

fn local_changes_origin(state: &AppState) -> PatchOrigin {
    let paths = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|root| {
            root.status
                .changes
                .iter()
                .filter(|c| state.ui.selected.contains(&root.canonical_key(c)))
                .map(|c| c.path.clone())
                .collect()
        })
        .unwrap_or_default();
    PatchOrigin::LocalChanges { paths }
}

pub fn from_comment(state: &mut AppState) {
    let name = patch_create::file_name(&state.ui.dlg.create_patch_comment.clone());
    state.ui.dlg.create_patch_name = name;
    open_settings(state);
}

pub fn open_settings(state: &mut AppState) {
    let location = state
        .ui
        .dlg
        .create_patch_root
        .as_ref()
        .map(|r| r.as_path().to_string_lossy().into_owned())
        .unwrap_or_default();
    state.ui.dlg.create_patch_location = location;
    sync(state);
    state.ui.dialog = Some(Dialog::PatchFileSettings);
}

pub fn set_root(state: &mut AppState, root: &RootId) {
    if state.ui.dlg.create_patch_root.as_ref() == Some(root) {
        return;
    }
    let Some(origin) = state.ui.dlg.create_patch_origin.clone() else {
        return;
    };
    reset(state, root.clone(), origin);
    sync(state);
}

fn reset(state: &mut AppState, root: RootId, origin: PatchOrigin) {
    let d = &mut state.ui.dlg;
    d.create_patch_root = Some(root);
    d.create_patch_origin = Some(origin);
    d.create_patch_comment.clear();
    d.create_patch_name.clear();
    d.create_patch_location.clear();
    d.create_patch_to_clipboard = false;
    d.create_patch_base_path.clear();
    d.create_patch_reverse = false;
    d.create_patch_encoding = PatchEncoding::default();
    d.create_patch_strip_leading = 0;
    d.create_patch_removable = 0;
    d.create_patch_raw = None;
    d.create_patch_error = None;
    state.create_patch_memos = Memos::default();
}

pub fn remove_leading_directory(state: &mut AppState) {
    state.ui.dlg.create_patch_strip_leading += 1;
    clamp_removal(state);
}

pub fn remove_all_leading_directories(state: &mut AppState) {
    state.ui.dlg.create_patch_strip_leading = state.ui.dlg.create_patch_removable;
}

pub fn restore_leading_directory(state: &mut AppState) {
    let at = &mut state.ui.dlg.create_patch_strip_leading;
    *at = at.saturating_sub(1);
}

pub fn restore_all_leading_directories(state: &mut AppState) {
    state.ui.dlg.create_patch_strip_leading = 0;
}

fn clamp_removal(state: &mut AppState) {
    let bound = state.ui.dlg.create_patch_removable;
    state.ui.dlg.create_patch_strip_leading = state.ui.dlg.create_patch_strip_leading.min(bound);
}

pub fn sync(state: &mut AppState) {
    let Some(origin) = state.ui.dlg.create_patch_origin.clone() else {
        return;
    };
    let (reverse, base, root) = {
        let d = &state.ui.dlg;
        (
            d.create_patch_reverse,
            d.create_patch_base_path.clone(),
            d.create_patch_root.clone(),
        )
    };
    let Some(root) = root else { return };
    if state.create_patch_memos.raw_key.as_ref() != Some(&(origin.clone(), reverse)) {
        match state.executor.patch_bytes(root.as_path(), &origin, reverse) {
            Ok(bytes) => {
                state.ui.dlg.create_patch_raw = Some(bytes);
                state.create_patch_memos.raw_key = Some((origin.clone(), reverse));
                state.ui.dlg.create_patch_error = None;
            }
            Err(e) => {
                state.ui.dlg.create_patch_raw = None;
                state.create_patch_memos.raw_key = Some((origin.clone(), reverse));
                state.ui.dlg.create_patch_error = Some(format!("Create patch: {e}"));
                return;
            }
        }
    }
    if state.create_patch_memos.limit_key.as_ref() != Some(&(origin.clone(), reverse, base.clone()))
    {
        let bound = state
            .ui
            .dlg
            .create_patch_raw
            .as_ref()
            .map(|raw| {
                patch_create::removable_leading_directories(&patch_create::rewrite_paths(
                    raw,
                    root.as_path(),
                    &base,
                    0,
                ))
            })
            .unwrap_or(0);
        state.ui.dlg.create_patch_removable = bound;
        clamp_removal(state);
        state.create_patch_memos.limit_key = Some((origin, reverse, base));
    }
}

/// The patch as the settings say it should be written: git's bytes, path-rewritten,
/// then encoded. `Err` is a refusal to write, never a partially transformed patch.
pub fn payload(state: &AppState) -> TgResult<Vec<u8>> {
    let d = &state.ui.dlg;
    let raw = d
        .create_patch_raw
        .as_deref()
        .filter(|raw| !raw.is_empty())
        .ok_or_else(|| TgError::Other("nothing to write — this change is empty".to_string()))?;
    let Some(root) = d.create_patch_root.as_ref() else {
        return Err(TgError::Other("no repository is in scope".to_string()));
    };
    let rewritten = patch_create::rewrite_paths(
        raw,
        root.as_path(),
        &d.create_patch_base_path,
        d.create_patch_strip_leading,
    );
    patch_create::encode(&rewritten, d.create_patch_encoding)
}

/// Parse failure is no preview rather than an error: a patch that does not parse is
/// still bytes this dialog can write, and `FILES THE PATCH NAMES` is a listing, not a
/// verdict. The reading of git's syntax happens here because the app layer owns that
/// engine call, the UI paints a value and never parses (`ADR-0022`).
pub fn preview(state: &AppState) -> Option<Patch> {
    let payload = payload(state).ok()?;
    let patch = turbogit_engine_api::patch::parse_patch(&String::from_utf8_lossy(&payload));
    (!patch.is_empty()).then_some(patch)
}

/// The settings dialog's affirmative: write the file, or hand the text to the frame
/// that owns the clipboard. A refusal leaves the dialog OPEN with its reason in it,
/// closing a dialog that failed is how a developer loses the setting that failed.
pub fn finish(state: &mut AppState) {
    let to_clipboard = state.ui.dlg.create_patch_to_clipboard;
    let payload = match payload(state) {
        Ok(payload) => payload,
        Err(e) => {
            state.ui.dlg.create_patch_error = Some(e.to_string());
            return;
        }
    };
    if to_clipboard {
        match patch_create::clipboard_text(&payload) {
            Ok(text) => {
                let d = &mut state.ui.dlg;
                d.create_patch_clipboard = Some(text);
                d.create_patch_error = None;
                state.ui.dialog = None;
            }
            Err(e) => {
                state.ui.dlg.create_patch_error = Some(e.to_string());
                return;
            }
        }
    } else {
        let written = destination(state);
        match std::fs::write(&written, &payload) {
            Ok(()) => {
                state.ui.dialog = None;
                report_created(state, &written);
            }
            Err(e) => {
                state.ui.dlg.create_patch_error = Some(format!(
                    "`{}` could not be written — {e}",
                    written.display()
                ));
                return;
            }
        }
    }
    state.ui.dlg.create_patch_origin = None;
}

fn report_created(state: &mut AppState, written: &Path) {
    let root = state.ui.dlg.create_patch_root.clone();
    let name = written.display_name_lossy_or(&written.to_string_lossy());
    let affected = root.clone().map(Affected::Root).unwrap_or(Affected::All);
    state.ui.activity.push(ActivityEntry {
        at: Local::now(),
        repo: crate::settle::activity_repo_label(&affected),
        message: format!("Patch created · {name}"),
        kind: ActivityKind::Success,
    });
    match state.settings.when_patch_is_created {
        PatchCreatedPolicy::Ask => {
            if let Some(root) = root {
                state.ui.patch_created = Some(PatchCreated {
                    path: written.to_path_buf(),
                    root,
                });
            } else {
                state.show_toast(Toast::success(format!("Wrote {name}")));
            }
        }
        PatchCreatedPolicy::ShowInFiles => {
            if let Some(root) = root {
                show_created(state, written, root);
            } else {
                state.show_toast(Toast::success(format!("Wrote {name}")));
            }
        }
        PatchCreatedPolicy::DoNothing => {}
    }
}

fn show_created(state: &mut AppState, written: &Path, root: RootId) {
    crate::apply_patch::open(state, PatchSource::File);
    if state.ui.dialog != Some(Dialog::ApplyPatch) {
        return;
    }
    crate::apply_patch::set_root(state, &root);
    state.ui.dlg.apply_patch_path = written.to_string_lossy().into_owned();
}

pub fn apply_created(state: &mut AppState) {
    let Some(PatchCreated { path, root }) = state.ui.patch_created.take() else {
        return;
    };
    show_created(state, &path, root);
}

pub fn dismiss_created(state: &mut AppState) {
    state.ui.patch_created = None;
}

pub fn import_created_to_shelf(state: &mut AppState) {
    let Some(PatchCreated { path, root }) = state.ui.patch_created.clone() else {
        return;
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported patch".to_string());
    match std::fs::read_to_string(&path)
        .map_err(TgError::Io)
        .and_then(|text| state.import_patch_to_shelf(&root, &name, &text))
    {
        Ok(shelf) => {
            state.ui.patch_created = None;
            state.show_toast(Toast::success(format!(
                "'{}' is on the shelf — nothing was applied",
                shelf.name
            )));
        }
        Err(e) => {
            state.ui.patch_created = None;
            state.last_error = Some(e.to_string());
            state.show_toast(Toast::error(e.to_string()));
        }
    }
}

/// `Copy as Patch to Clipboard`: the same generation with **no dialog at all**,
/// default location, default base, no removal, no reversal, UTF-8.
pub fn copy_to_clipboard(state: &mut AppState, origin: PatchOrigin) {
    let Some(root) = state.selected_root.clone() else {
        state.show_toast(Toast::info(
            "Copy as patch needs a repository: open a workspace first.",
        ));
        return;
    };
    let payload = state
        .executor
        .patch_bytes(root.as_path(), &origin, false)
        .and_then(|raw| {
            if raw.is_empty() {
                return Err(TgError::Other(
                    "this change is empty, so there is no patch to copy".to_string(),
                ));
            }
            Ok(patch_create::rewrite_paths(&raw, root.as_path(), "", 0))
        })
        .and_then(|bytes| patch_create::clipboard_text(&bytes));
    match payload {
        Ok(text) => {
            state.ui.dlg.create_patch_clipboard = Some(text);
            state.show_toast(Toast::success("Patch copied to the clipboard"));
        }
        Err(e) => state.show_toast(Toast::error(e.to_string())),
    }
}

pub fn take_clipboard(state: &mut AppState) -> Option<String> {
    state.ui.dlg.create_patch_clipboard.take()
}

pub fn choose_location(state: &mut AppState) {
    let Some(writer) = state.patch_writer.take() else {
        state.show_toast(Toast::info("Type the patch file's path instead."));
        return;
    };
    let chosen = writer();
    state.patch_writer = Some(writer);
    let Some(path) = chosen else { return };
    if let Some(dir) = path.parent() {
        state.ui.dlg.create_patch_location = dir.to_string_lossy().into_owned();
    }
    if let Some(name) = path.file_name() {
        state.ui.dlg.create_patch_name = name.to_string_lossy().into_owned();
    }
}

fn destination(state: &AppState) -> PathBuf {
    let location = state.ui.dlg.create_patch_location.trim();
    let name = if state.ui.dlg.create_patch_name.trim().is_empty() {
        "patch.patch"
    } else {
        state.ui.dlg.create_patch_name.trim()
    };
    if location.is_empty() {
        return PathBuf::from(name);
    }
    let dir = Path::new(location);
    if location.ends_with(".patch") || name.contains('/') || name.contains('\\') {
        if dir.is_dir() {
            return dir.join(name);
        }
        return PathBuf::from(location);
    }
    dir.join(name)
}

fn origin_subject(state: &AppState, root: &RootId) -> String {
    let Some(origin) = state.ui.dlg.create_patch_origin.as_ref() else {
        return String::new();
    };
    let commit = match origin {
        PatchOrigin::Commit { id } => id.clone(),
        PatchOrigin::FileRevision { commit, .. } => commit.clone(),
        PatchOrigin::LocalChanges { .. } => return String::new(),
    };
    state
        .caches
        .log(root)
        .and_then(|commits| commits.iter().find(|c| c.id == commit))
        .map(|c| c.message.lines().next().unwrap_or_default().to_string())
        .unwrap_or_default()
}
