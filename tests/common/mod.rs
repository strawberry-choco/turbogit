//! Reusable headless-shell harness helpers (issue #9).
//!
//! Shared by `shell_frame.rs` and the other issue-named test files so each
//! ticket can drive the full [`turbogit::ui::render`] shell without copying
//! setup code. The harness runs over synthetic raw input (egui_kittest, no
//! GPU / window / display server) and asserts only on public surfaces:
//! - **Painted output** — the frame's shapes from `FullOutput`, i.e. exactly
//!   what a software painter would fill (text galleys carry their strings;
//!   filled rects carry their geometry + token color).
//! - **State transitions** — public `AppState` fields after the frames.

// Each integration-test crate compiles its own copy of this module and uses
// only the helpers it needs; the rest are intentionally unused there.
#![allow(dead_code)]

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use turbogit::engine::GitExecutor;
use turbogit::state::AppState;
use turbogit::theme::{configure_style, install_fonts};

/// A harness rendering the full shell over a fresh [`AppState`].
///
/// The project dir is an empty temp directory: zero roots are discovered, so
/// the render is deterministic, no background git workers are spawned, and —
/// per the Welcome-vs-shell model (spec §9.2) — the central body shows the
/// Welcome placeholder while every shell region still renders.
/// Setup mirrors production (`app.rs`): dark-only tokens every frame plus
/// embedded JetBrains Mono installed once.
pub fn shell_harness() -> (Harness<'static, AppState>, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("temp project dir");
    // Inject an empty throwaway config dir so the developer's real global
    // recents file never leaks into headless tests (ADR-0005 test seam —
    // `AppState` docs: "Tests inject a temp dir so the real user
    // configuration is never touched"). Deliberately leaked: the harness
    // outlives this function, and a deleted config dir would make later
    // recents reads/writes racy.
    let cfg = tempfile::tempdir().expect("temp config dir");
    let cfg_path = cfg.path().to_path_buf();
    std::mem::forget(cfg);
    let state = AppState::launch_in(Some(project.path().to_path_buf()), Some(cfg_path));
    assert!(
        state.multi.roots.is_empty(),
        "test project must discover no roots"
    );

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    (harness, project)
}

/// All text painted by the last completed frame.
pub fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

/// Assert `needle` appears in some painted text galley.
#[track_caller]
pub fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Assert `needle` appears in no painted text galley.
#[track_caller]
pub fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

/// Paint-time origin of the first text galley painting exactly `text`.
///
/// Exact matching keeps distinct labels unambiguous ("Log" vs "Git Log").
/// Used to relate a label to the region that visually contains it (e.g. the
/// active tab's surface rect).
pub fn galley_origin(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
}

/// Every filled rectangle painted by the last frame as `(rect, fill)`.
///
/// Panel frames, toolbars, rails, tabs, and buttons all emit `Shape::Rect`
/// fills, which makes spec-dimension assertions possible without reaching
/// into egui internals.
pub fn filled_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape) if rect_shape.fill != Color32::TRANSPARENT => {
                Some((rect_shape.rect, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

/// Step frames until the painted output stabilizes.
///
/// The first frames after startup relayout (embedded fonts take effect at
/// pass 2), so queries and clicks must only happen on a settled frame —
/// mirroring a user clicking an already-rendered shell.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("shell layout did not settle within 10 frames");
}

// --- Recording executor (issue #21) -----------------------------------------
//
// `engine::fake` is unit-test-only (`#[cfg(test)]`), so integration tests
// assert flag selection at the executor boundary through this transparent
// wrapper: every call delegates to a real inner engine while push /
// push-dry-run invocations are recorded verbatim (remote, branch, force).

/// One recorded mutating call at the executor boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCall {
    Push {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
    },
    PushDryRun {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
    },
    ApplyPatch {
        direction: turbogit::engine::ApplyDirection,
    },
    AddIntentToAdd(Vec<PathBuf>),
    Add(Vec<PathBuf>),
    CommitAll,
    CommitIndex,
}

/// Delegating [`GitExecutor`] that records push / dry-run calls.
pub struct RecordingExecutor {
    pub inner: Arc<dyn GitExecutor>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl RecordingExecutor {
    pub fn new(inner: Arc<dyn GitExecutor>) -> Self {
        Self {
            inner,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Snapshot of every recorded call so far, in order.
    pub fn recorded(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("calls mutex").clone()
    }

    /// True once a `Push` with exactly these fields has been recorded.
    pub fn contains_push(&self, remote: &str, branch: &str, force: bool) -> bool {
        self.recorded().iter().any(|c| match c {
            RecordedCall::Push {
                remote: r,
                branch: b,
                force: f,
                ..
            } => r == remote && b == branch && *f == force,
            _ => false,
        })
    }

    /// True once a `PushDryRun` with exactly these fields has been recorded.
    pub fn contains_dry_run(&self, remote: &str, branch: &str, force: bool) -> bool {
        self.recorded().iter().any(|c| match c {
            RecordedCall::PushDryRun {
                remote: r,
                branch: b,
                force: f,
                ..
            } => r == remote && b == branch && *f == force,
            _ => false,
        })
    }
}

impl GitExecutor for RecordingExecutor {
    fn status(&self, root: &Path) -> turbogit::error::TgResult<turbogit::model::RootStatus> {
        self.inner.status(root)
    }

    fn log(
        &self,
        root: &Path,
        opts: &turbogit::model::LogOpts,
    ) -> turbogit::error::TgResult<Vec<turbogit::model::Commit>> {
        self.inner.log(root, opts)
    }

    fn ref_decorations(
        &self,
        root: &Path,
    ) -> turbogit::error::TgResult<Vec<(turbogit::model::CommitId, Vec<turbogit::model::CommitRef>)>>
    {
        self.inner.ref_decorations(root)
    }

    fn commit_files(
        &self,
        root: &Path,
        commit: &str,
    ) -> turbogit::error::TgResult<Vec<turbogit::model::Change>> {
        self.inner.commit_files(root, commit)
    }

    fn branches(&self, root: &Path) -> turbogit::error::TgResult<Vec<turbogit::model::Branch>> {
        self.inner.branches(root)
    }

    fn current_branch(&self, root: &Path) -> turbogit::error::TgResult<Option<String>> {
        self.inner.current_branch(root)
    }

    fn ahead_behind(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> turbogit::error::TgResult<(usize, usize)> {
        self.inner.ahead_behind(root, branch, upstream)
    }

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> turbogit::error::TgResult<Vec<turbogit::model::CommitId>> {
        self.inner.outgoing_commits(root, branch, upstream)
    }

    fn remotes(&self, root: &Path) -> turbogit::error::TgResult<Vec<turbogit::model::Remote>> {
        self.inner.remotes(root)
    }

    fn stash_list(&self, root: &Path) -> turbogit::error::TgResult<Vec<turbogit::model::Stash>> {
        self.inner.stash_list(root)
    }

    fn worktree_list(
        &self,
        root: &Path,
    ) -> turbogit::error::TgResult<Vec<turbogit::model::Worktree>> {
        self.inner.worktree_list(root)
    }

    fn submodule_paths(&self, root: &Path) -> turbogit::error::TgResult<Vec<PathBuf>> {
        self.inner.submodule_paths(root)
    }

    fn config_get(&self, root: &Path, key: &str) -> turbogit::error::TgResult<Option<String>> {
        self.inner.config_get(root, key)
    }

    fn init(&self, root: &Path) -> turbogit::error::TgResult<()> {
        self.inner.init(root)
    }

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> turbogit::error::TgResult<()> {
        // `clone` collides with `Clone::clone`; disambiguate via the trait.
        GitExecutor::clone(&*self.inner, url, dest, depth)
    }

    fn add_remote(&self, root: &Path, name: &str, url: &str) -> turbogit::error::TgResult<()> {
        self.inner.add_remote(root, name, url)
    }

    fn fetch(&self, root: &Path, remote: Option<&str>) -> turbogit::error::TgResult<()> {
        self.inner.fetch(root, remote)
    }

    fn pull(&self, root: &Path, rebase: bool) -> turbogit::error::TgResult<()> {
        self.inner.pull(root, rebase)
    }

    fn push(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> turbogit::error::TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Push {
                root: root.to_path_buf(),
                remote: remote.to_string(),
                branch: branch.to_string(),
                force,
            });
        self.inner.push(root, remote, branch, force)
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> turbogit::error::TgResult<String> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::PushDryRun {
                root: root.to_path_buf(),
                remote: remote.to_string(),
                branch: branch.to_string(),
                force,
            });
        self.inner.push_dry_run(root, remote, branch, force)
    }

    fn commit(
        &self,
        root: &Path,
        message: &str,
        amend: bool,
    ) -> turbogit::error::TgResult<turbogit::model::CommitId> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::CommitAll);
        self.inner.commit(root, message, amend)
    }

    fn commit_index(
        &self,
        root: &Path,
        message: &str,
        amend: bool,
    ) -> turbogit::error::TgResult<turbogit::model::CommitId> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::CommitIndex);
        self.inner.commit_index(root, message, amend)
    }

    fn merge(
        &self,
        root: &Path,
        target: &str,
        opts: &turbogit::model::MergeOpts,
    ) -> turbogit::error::TgResult<()> {
        self.inner.merge(root, target, opts)
    }

    fn rebase(
        &self,
        root: &Path,
        onto: &str,
        opts: &turbogit::model::RebaseOpts,
    ) -> turbogit::error::TgResult<()> {
        self.inner.rebase(root, onto, opts)
    }

    fn cherry_pick(&self, root: &Path, commit: &str) -> turbogit::error::TgResult<()> {
        self.inner.cherry_pick(root, commit)
    }

    fn abort(&self, root: &Path, op: &str) -> turbogit::error::TgResult<()> {
        self.inner.abort(root, op)
    }

    fn continue_op(&self, root: &Path, op: &str) -> turbogit::error::TgResult<()> {
        self.inner.continue_op(root, op)
    }

    fn rebase_interactive(
        &self,
        root: &Path,
        plan: &[turbogit::model::RebasePlanEntry],
    ) -> turbogit::error::TgResult<()> {
        self.inner.rebase_interactive(root, plan)
    }

    fn stash_push(
        &self,
        root: &Path,
        message: &str,
        keep_index: bool,
    ) -> turbogit::error::TgResult<()> {
        self.inner.stash_push(root, message, keep_index)
    }

    fn stash_pop(&self, root: &Path, index: usize) -> turbogit::error::TgResult<()> {
        self.inner.stash_pop(root, index)
    }

    fn stash_drop(&self, root: &Path, index: usize) -> turbogit::error::TgResult<()> {
        self.inner.stash_drop(root, index)
    }

    fn worktree_add(
        &self,
        root: &Path,
        path: &Path,
        branch: &str,
    ) -> turbogit::error::TgResult<()> {
        self.inner.worktree_add(root, path, branch)
    }

    fn add(&self, root: &Path, paths: &[PathBuf]) -> turbogit::error::TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Add(paths.to_vec()));
        self.inner.add(root, paths)
    }

    fn add_all(&self, root: &Path) -> turbogit::error::TgResult<()> {
        self.inner.add_all(root)
    }

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> turbogit::error::TgResult<()> {
        self.inner.unstage(root, paths)
    }

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> turbogit::error::TgResult<()> {
        self.inner.restore(root, paths)
    }

    fn apply_patch_to_index(
        &self,
        _root: &Path,
        _patch: &str,
        direction: turbogit::engine::ApplyDirection,
    ) -> turbogit::error::TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::ApplyPatch { direction });
        self.inner.apply_patch_to_index(_root, _patch, direction)
    }

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> turbogit::error::TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::AddIntentToAdd(paths.to_vec()));
        self.inner.add_intent_to_add(root, paths)
    }

    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> turbogit::error::TgResult<()> {
        self.inner.branch_create(root, name, checkout, start_point)
    }

    fn branch_checkout(&self, root: &Path, name: &str) -> turbogit::error::TgResult<()> {
        self.inner.branch_checkout(root, name)
    }

    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> turbogit::error::TgResult<()> {
        self.inner.branch_delete(root, name, force)
    }

    fn branch_delete_remote(
        &self,
        root: &Path,
        remote: &str,
        name: &str,
    ) -> turbogit::error::TgResult<()> {
        self.inner.branch_delete_remote(root, remote, name)
    }

    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> turbogit::error::TgResult<()> {
        self.inner.branch_rename(root, old, new)
    }

    fn tag_create(
        &self,
        root: &Path,
        name: &str,
        message: Option<&str>,
    ) -> turbogit::error::TgResult<()> {
        self.inner.tag_create(root, name, message)
    }

    fn tag_list(&self, root: &Path) -> turbogit::error::TgResult<Vec<String>> {
        self.inner.tag_list(root)
    }

    fn tag_checkout(&self, root: &Path, name: &str) -> turbogit::error::TgResult<()> {
        self.inner.tag_checkout(root, name)
    }

    fn tag_push(
        &self,
        root: &Path,
        remote: &str,
        name: Option<&str>,
        all: bool,
    ) -> turbogit::error::TgResult<()> {
        self.inner.tag_push(root, remote, name, all)
    }

    fn diff(
        &self,
        root: &Path,
        opts: &turbogit::model::DiffOpts,
    ) -> turbogit::error::TgResult<String> {
        self.inner.diff(root, opts)
    }

    fn blame(
        &self,
        root: &Path,
        path: &Path,
        rev: Option<&str>,
    ) -> turbogit::error::TgResult<Vec<turbogit::model::BlameLine>> {
        self.inner.blame(root, path, rev)
    }

    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> turbogit::error::TgResult<String> {
        self.inner.show_file(root, rev, path)
    }

    fn show_file_bytes(
        &self,
        root: &Path,
        rev: &str,
        path: &Path,
    ) -> turbogit::error::TgResult<Vec<u8>> {
        self.inner.show_file_bytes(root, rev, path)
    }

    fn revert(&self, root: &Path, commit: &str) -> turbogit::error::TgResult<()> {
        self.inner.revert(root, commit)
    }

    fn undo_last_commit(&self, root: &Path) -> turbogit::error::TgResult<()> {
        self.inner.undo_last_commit(root)
    }

    fn stash_apply(&self, root: &Path, index: usize) -> turbogit::error::TgResult<()> {
        self.inner.stash_apply(root, index)
    }

    fn is_repo(&self, path: &Path) -> bool {
        self.inner.is_repo(path)
    }
}
