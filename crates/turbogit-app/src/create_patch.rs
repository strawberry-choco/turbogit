//! The `Create patch` flow — take an origin, get git's bytes, rewrite paths,
//! encode, write or copy, then honour `When patch is created:` — as free
//! functions over [`AppState`], the shape [`crate::granular`], [`crate::settle`]
//! and [`crate::apply_patch`] already take. The dialogs' storage stays in
//! [`crate::state::DialogState`]; this module owns the policy over it and the UI
//! module keeps painting.
//!
//! Two memos keep the per-frame [`sync`] from asking git once per frame: git's
//! bytes are re-read only when the origin or `Reverse patch` moves, and the
//! removal bound is re-measured only when the base path moves too. They live in
//! the module-private [`Memos`] because they are this rule's bookkeeping rather
//! than the dialog's payload — the raw bytes they describe stay in the dialog.

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

/// The two memos behind [`sync`], private to this module: the origin and reverse
/// flag git's bytes were last generated from, and the origin, reverse flag and
/// base path the removal bound was last measured under.
///
/// They are not the dialog's payload — the bytes they describe stay in
/// [`crate::state::DialogState`] — so they are held beside the rule that reads
/// them rather than in the field group they would otherwise sit among.
#[derive(Default)]
pub(crate) struct Memos {
    raw_key: Option<(PatchOrigin, bool)>,
    limit_key: Option<(PatchOrigin, bool, String)>,
}

/// Open the create flow for `origin`: the comment step first when the source has
/// no name of its own, `Patch File Settings` when it does.
///
/// A commit and a file revision are named by the commit they come from, and the
/// reference's comment step asks for the comment that a commit already has — so
/// those routes open the settings directly, seeded from the commit's own subject.
/// Local changes have no subject, which is exactly what the step is for.
///
/// The commit's subject is read from the **cached log**, the same value the details
/// pane shows and Copy message copies; a create route that spawned its own `git
/// show` would be the uncached read ADR-0021 exists to prevent.
pub fn open(state: &mut AppState, origin: PatchOrigin) {
    let Some(root) = state.selected_root.clone() else {
        state.show_toast(Toast::info(
            "Create patch needs a repository: open a workspace first.",
        ));
        return;
    };
    open_on(state, root, origin);
}

/// [`open`] with the root named — the form for a caller that already knows which
/// tree it means, which is every row-scoped route: a Log commit and a file revision
/// both carry their row's root, and taking the focused root at the last moment would
/// generate a patch of a different repository than the one the developer clicked.
pub fn open_on(state: &mut AppState, root: RootId, origin: PatchOrigin) {
    let needs_comment = origin.needs_comment();
    reset(state, root.clone(), origin.clone());
    if needs_comment {
        // Seeded from the Commit window's own message box when the developer has
        // already written the comment they are about to ship: the step asks for the
        // commit comment, and one typed seconds earlier is that comment.
        state.ui.dlg.create_patch_comment = state.ui.commit_message.clone();
        state.ui.dialog = Some(Dialog::CreatePatchComment);
        return;
    }
    let subject = origin_subject(state, &root);
    state.ui.dlg.create_patch_comment = subject.clone();
    state.ui.dlg.create_patch_name = patch_create::file_name(&subject);
    open_settings(state);
}

/// `Create Patch from Local Changes` as the Commit window reaches it: the checked
/// files when any are checked — the reference's "file or changelist" — and every
/// local change of this root when nothing is.
///
/// The checked set is read here rather than at the write, so the patch and the
/// Commit window's own box state describe the same files.
pub fn open_from_selection(state: &mut AppState) {
    let origin = local_changes_origin(state);
    open(state, origin);
}

/// [`open_from_selection`]'s verb with **no dialog at all**.
pub fn copy_from_selection(state: &mut AppState) {
    let origin = local_changes_origin(state);
    copy_to_clipboard(state, origin);
}

/// The focused root's local changes as an origin: `paths` empty is the whole set,
/// and non-empty is exactly the files the developer included.
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

/// The comment step's answer: the comment names the file, and the settings step
/// opens on it. An empty comment is not refused — the name simply falls back to
/// the artifact's own name, which the dialog shows before it is used.
pub fn from_comment(state: &mut AppState) {
    let name = patch_create::file_name(&state.ui.dlg.create_patch_comment.clone());
    state.ui.dlg.create_patch_name = name;
    open_settings(state);
}

/// Open `Patch File Settings` on the payload as it now stands: the default
/// location is the repository the patch is generated FROM, and git is asked for
/// the patch itself on the first sync rather than here, so opening a window and
/// changing one field cost the same one read.
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

/// Point the create flow at `root`. The patch, its base path and its removal count
/// are all facts about ONE working tree, so switching root
/// drops the answer measured against the other tree rather than painting it here.
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

/// Start the create group again for one visit to one root. Everything the last
/// patch worked out for itself — the removal count, the verdict on the bytes, the
/// error — belongs to that patch and is cleared here rather than at the frame that
/// paints it.
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

/// `Remove Leading Directory`, `Remove All Leading Directories`, and their two
/// restores: the counting repair pair, driven as four functions on one field.
///
/// `Restore` is one step at a time, which is what makes the bracketed number worth
/// reading; every removal is held by [`clamp_removal`].
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

/// The one statement of the removal clamp: the count never grows past the bound
/// [`crate::state::DialogState::create_patch_removable`] holds — the shallowest
/// path's directory count — because a removal applies to every path in the patch,
/// and a count that erased `README.md`'s name would produce a patch that names
/// nothing rather than a shorter path. Both places the count can run ahead of that
/// bound — the per-press increment and the base path moving the bound down — end
/// here.
fn clamp_removal(state: &mut AppState) {
    let bound = state.ui.dlg.create_patch_removable;
    state.ui.dlg.create_patch_strip_leading = state.ui.dlg.create_patch_strip_leading.min(bound);
}

/// Bring the generated patch and the removal bound in line with what the developer
/// has since typed, pressed or switched. Runs once per frame, and asks git only when
/// the ORIGIN or `Reverse patch` moved: a base path is a statement about the paths
/// this application rewrites afterwards, so it cannot change what git was asked.
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
        // The bound shrinking must not leave the count claiming directories that
        // are no longer in the paths.
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

/// The patch as the settings describe it, read back into the value the surface paints.
///
/// Parse failure is no preview rather than an error: a patch that does not parse is
/// still bytes this dialog can write, and `FILES THE PATCH NAMES` is a listing, not a
/// verdict. The reading of git's syntax happens here because the app layer owns that
/// engine call — the UI paints a value and never parses (`ADR-0022`).
pub fn preview(state: &AppState) -> Option<Patch> {
    let payload = payload(state).ok()?;
    let patch = turbogit_engine_api::patch::parse_patch(&String::from_utf8_lossy(&payload));
    (!patch.is_empty()).then_some(patch)
}

/// The settings dialog's affirmative: write the file, or hand the text to the frame
/// that owns the clipboard. A refusal leaves the dialog OPEN with its reason in it —
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
                // The toast arrives from the frame that drains the slot, because
                // until `copy_text` has run nothing has been copied.
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

/// What happens **after** a create route wrote a patch file: the durable feed
/// first, then the `When patch is created:` arm the developer chose.
///
/// The feed entry is written before the policy is read and is not gated by it,
/// because the durable record is not part of what a policy arm may withhold:
/// `Do nothing` withholds the *transient* report — the toast, the ask, the window —
/// and cannot be allowed to also erase the record. The two asking and showing arms
/// are told apart by what they leave armed, never by whether the file was mentioned.
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
            // A patch generated outside every repository still has a file; what it
            // cannot have is a root to park it under, and an ask that could not
            // honour its own first verb would be a decoy. So the ask names the root
            // it came from, and there is no ask without one.
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

/// The `Show in {0}` arm. The reference reveals the file in the operating
/// system's file manager; this build has no reveal seam, so the created patch
/// goes to the one surface that reads a patch file and lists what it names —
/// recorded as a divergence rather than dropped into silence.
fn show_created(state: &mut AppState, written: &Path, root: RootId) {
    crate::apply_patch::open(state, PatchSource::File);
    if state.ui.dialog != Some(Dialog::ApplyPatch) {
        return;
    }
    crate::apply_patch::set_root(state, &root);
    state.ui.dlg.apply_patch_path = written.to_string_lossy().into_owned();
}

/// The ask's second verb: take the created file to the window that reads it.
pub fn apply_created(state: &mut AppState) {
    let Some(PatchCreated { path, root }) = state.ui.patch_created.take() else {
        return;
    };
    show_created(state, &path, root);
}

/// The ask's refusal: nothing is done with the patch, and the ask goes away. The
/// feed entry the write already made stays, which is what makes silence auditable.
pub fn dismiss_created(state: &mut AppState) {
    state.ui.patch_created = None;
}

/// The `When patch is created: Ask` follow-up's deferral verb: park the file the
/// create route just wrote, under the root it was generated from.
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

/// `Copy as Patch to Clipboard`: the same generation with **no dialog at all** —
/// default location, default base, no removal, no reversal, UTF-8.
///
/// The verb's whole point is that it asks nothing, so it cannot open a window, and
/// it reads git directly rather than borrowing a dialog's memo: the clipboard route
/// and the dialog route are two callers of one engine read, not two patches.
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

/// Take the pending clipboard payload. The UI frame that owns an `egui::Context`
/// drains this and calls `copy_text`, which is the app's only clipboard write and
/// the reason it is a slot rather than a call from `AppState`.
pub fn take_clipboard(state: &mut AppState) -> Option<String> {
    state.ui.dlg.create_patch_clipboard.take()
}

/// The native save-dialog seam: the developer's chosen file replaces the typed
/// location and name. Declining writes nothing and changes nothing.
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

/// Where the settings dialog writes: the location field as a directory, plus the
/// name field — which is the comment's first line unless the developer renamed it.
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
    // A location that already IS the chosen file — which is what the native save
    // dialog answers with — must not be joined into itself.
    if location.ends_with(".patch") || name.contains('/') || name.contains('\\') {
        if dir.is_dir() {
            return dir.join(name);
        }
        return PathBuf::from(location);
    }
    dir.join(name)
}

/// The commit subject the patch's file name is seeded from, read from the cached
/// log for the origin's own commit. `None` when the cache does not hold it, which
/// leaves the name empty rather than inventing one.
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
