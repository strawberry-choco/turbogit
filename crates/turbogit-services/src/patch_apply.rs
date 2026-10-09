//! Applying a patch to the working tree, with a rollback that is **total** or
//! says precisely where it is not.
//!
//! The guarantee this module exists to hold is the one `Apply patch` makes: a
//! failed apply leaves the working tree exactly as it was. It is held by
//! recording the bytes every path the patch names holds BEFORE git is asked to
//! write anything, and putting those bytes back when a section fails. The record
//! is the whole of the mechanism — see [`apply_patch_with`].
//!
//! The module sits at the service seam and touches git only through
//! [`GitExecutor`]: [`GitExecutor::check_patch`] answers the preflight,
//! [`GitExecutor::apply_patch_to_worktree`] writes. Restoring is not git's
//! business at all — it is a plain write of recorded bytes, which is why the
//! rollback is [`Rollback`] rather than a port method, and why the state where
//! restoring fails is a filesystem outcome that has to be *named* rather than a
//! git error that could be guessed at. The crate header's "no I/O outside the
//! engine seam" already counts one such write — `integrate_service`'s
//! `Get from Branch` puts a file's contents into the working tree the same way —
//! and a rollback is the same act in the other direction.

use std::path::{Component, Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{Patch, PatchFile};
use turbogit_engine_api::GitExecutor;

/// One path the patch names, and what it held before the apply.
///
/// `None` content is **absence**, recorded as a fact rather than as an empty
/// file: the rollback of a create is a removal, and writing a zero-length file
/// over a path that did not exist would leave behind an untracked empty file the
/// developer never had — a tree that is not "exactly as it was".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    /// Repo-relative, exactly as the patch section names it.
    pub path: PathBuf,
    pub content: Option<Vec<u8>>,
}

/// A path the rollback could not put back.
///
/// Carries the recorded bytes rather than a pointer to them: the retry of a
/// failed restore happens after the apply is over, possibly after the dialog has
/// been re-painted a hundred times, and the content it writes must still be the
/// content the tree held. Dropping it would make the recovery action a lie.
#[derive(Clone, Debug)]
pub struct Unrestored {
    pub path: PathBuf,
    /// Why this one write failed, in the filesystem's words.
    pub reason: String,
    pub recorded: Option<Vec<u8>>,
}

impl Unrestored {
    /// The feed's and the dialog's line for one unrestorable path: the path
    /// first, because that is what the developer has to act on, and the reason
    /// beside it because "could not be restored" without a cause is not a
    /// disclosure.
    pub fn stated(&self) -> String {
        format!("{} — {}", self.path.display(), self.reason)
    }
}

/// How an apply ended. Three outcomes, because two would be a promise the
/// rollback cannot keep: the case where the rollback itself fails has to be
/// sayable separately from the case where it did not.
#[derive(Clone, Debug)]
pub enum ApplyOutcome {
    /// Every section landed. `files` are the paths the patch named.
    Applied { paths: Vec<PathBuf> },
    /// The apply failed and **every** recorded path is back where it was.
    Aborted {
        reason: String,
        restored: Vec<PathBuf>,
    },
    /// The apply failed and at least one path could not be put back.
    RollbackFailed {
        reason: String,
        unrestored: Vec<Unrestored>,
        /// The paths that WERE restored, named so the developer is not left to
        /// guess which half of the tree is now trustworthy.
        restored: Vec<PathBuf>,
    },
}

impl ApplyOutcome {
    /// The two titles the surface ships, named here because the window that paints
    /// them and the feed that remembers them must not be able to disagree.
    pub const ABORTED_TITLE: &'static str = "Apply patch aborted";
    /// The state that makes the aborted case's promise honest: an apply that failed
    /// is only ever "all files rolled back" when the rollback did not itself fail,
    /// and the failure needs a name of its own rather than a louder toast.
    pub const ROLLBACK_FAILED_TITLE: &'static str = "Rollback failed";
    /// The guarantee itself, in the reference's own words. Only the `Aborted`
    /// variant can say it, which is the whole reason `RollbackFailed` exists as a
    /// third variant instead of as a detail inside the second.
    pub const ABORTED_MESSAGE: &'static str =
        "All files changed during 'Apply Patch' action were rolled back";

    /// Whether the working tree is back in the state the developer left it.
    pub fn tree_is_restored(&self) -> bool {
        !matches!(self, Self::RollbackFailed { .. })
    }

    /// The state's name, and the one place the two shipped strings live.
    ///
    /// Here rather than at a call site because **the dialog that paints the
    /// outcome and the activity entry that remembers it are two readers of one
    /// fact**, and a `Rollback failed` the notification words differently from
    /// the feed is a state the product holds in two dialects. Prose on an outcome
    /// enum is the app's existing convention for exactly this pair — see
    /// `turbogit_app::operation::LifecycleVerb::label`.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Applied { .. } => "Patch applied",
            Self::Aborted { .. } => Self::ABORTED_TITLE,
            Self::RollbackFailed { .. } => Self::ROLLBACK_FAILED_TITLE,
        }
    }

    /// The state's sentence. The aborted case's is the guarantee itself, stated
    /// as the reference states it, because it is the reason the third outcome has
    /// to exist separately: it is only true when it is true.
    pub fn message(&self) -> String {
        match self {
            Self::Applied { paths } => format!(
                "{} file(s) changed in the working tree. Nothing was staged and nothing was \
                 committed.",
                paths.len()
            ),
            Self::Aborted { restored, .. } => {
                if restored.is_empty() {
                    "Nothing was changed: the apply stopped before it wrote any file.".to_string()
                } else {
                    Self::ABORTED_MESSAGE.to_string()
                }
            }
            Self::RollbackFailed { unrestored, .. } => format!(
                "The patch did not apply, and {} file(s) could not be restored to the content \
                 they held before it. They are named below, and the change is still in your \
                 working tree.",
                unrestored.len()
            ),
        }
    }

    /// The paths this outcome says something about — applied, restored, or not.
    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            Self::Applied { paths } => paths.clone(),
            Self::Aborted { restored, .. } => restored.clone(),
            Self::RollbackFailed {
                unrestored,
                restored,
                ..
            } => {
                let mut all = restored.clone();
                all.extend(unrestored.iter().map(|u| u.path.clone()));
                all
            }
        }
    }
}

/// The one write a rollback performs.
///
/// A seam rather than an inline `std::fs` call for an honest reason: the state
/// this module exists to name is the one where restoring fails, and a suite that
/// cannot make a write fail cannot assert that state is reachable — it can only
/// assert that the code contains the word. The production implementation is
/// [`Worktree`]; nothing in production passes anything else.
pub trait Rollback {
    /// Put `content` back at `path`; `None` means the path must not exist.
    fn put(&mut self, path: &Path, content: Option<&[u8]>) -> TgResult<()>;
}

/// The production [`Rollback`]: the filesystem, as the record says.
pub struct Worktree;

impl Rollback for Worktree {
    fn put(&mut self, path: &Path, content: Option<&[u8]>) -> TgResult<()> {
        match content {
            Some(bytes) => {
                std::fs::write(path, bytes)?;
                Ok(())
            }
            None => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                // Already absent is the recorded state, not a failure.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            },
        }
    }
}

/// The paths one patch touches, in a stable order, deduplicated.
///
/// Both sides of a **rename section** are listed: git's one section moves a path,
/// and the rollback has to put the old name back *and* remove the new one, so a
/// reader that collected only the destination would leave the source behind with
/// its content half-moved.
pub fn touched_paths(patch: &Patch) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for file in &patch.files {
        for raw in [file.old_path.as_str(), file.new_path.as_str()] {
            if raw.is_empty() {
                continue;
            }
            let path = PathBuf::from(raw);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
}

/// Would this patch apply? `Show Diff` as a **preflight, not a confirmation**
/// (the reference's own words: "to check if your patch can be applied without
/// conflicts"): the answer is git's verdict on the tree as it stands, and the
/// call mutates nothing — `git apply --check` writes no file and stages nothing.
///
/// The conflicting lines it reports are git's own stderr, verbatim, because that
/// text is the signal (the same rule [`GitExecutor::check_patch`] states for the
/// cherry-pick forecast); what this adds is the patch's path list, so a surface
/// can say which of the lines name a file the developer is about to touch.
#[derive(Clone, Debug)]
pub enum Preflight {
    /// It applies. `files` is how many sections git was asked about.
    Clean { files: usize },
    /// It does not. `report` is git's verdict; `paths` are the patch's own.
    Conflicts { report: String, paths: Vec<PathBuf> },
    /// Nothing to ask git about, or a patch this module will not run for the
    /// reason stated in `report`. The third case exists because a refusal to
    /// start and a clean answer are different answers.
    Refused { report: String },
}

/// The preflight's verdict on `patch` in `root`.
pub fn preflight(vcs: &dyn GitExecutor, root: &Path, patch: &Patch) -> Preflight {
    let paths = touched_paths(patch);
    if let Err(report) = startable(patch, &paths) {
        return Preflight::Refused { report };
    }
    match vcs.check_patch(root, patch) {
        Ok(()) => Preflight::Clean {
            files: patch.files.len(),
        },
        Err(e) => Preflight::Conflicts {
            report: e.to_string(),
            paths,
        },
    }
}

/// Apply `patch` to `root`'s working tree, rolling the whole thing back if any
/// section fails. Production entry point; see [`apply_patch_with`] for the seam.
pub fn apply_patch(vcs: &dyn GitExecutor, root: &Path, patch: &Patch) -> ApplyOutcome {
    apply_patch_with(vcs, root, patch, &mut Worktree)
}

/// The apply, with the rollback's write made injectable.
///
/// **Why the record comes first and git comes second, and why the sections go
/// one at a time.** `git apply` over the whole patch is itself all-or-nothing, so
/// a single call would make the rollback unreachable in the common case — and
/// would be the wrong shape anyway, because the tree this patch lands on is not
/// frozen: a later section can fail on state an earlier section just wrote, on a
/// path that appeared in between, or on the filesystem refusing the second write
/// after the first succeeded. Per-section application is what makes "the first
/// file was already changed when the second one failed" a case the code actually
/// handles rather than one it merely does not have. The cost is one `git` process
/// per section, paid only by a surface that has asked for a guarantee.
///
/// The rollback restores every recorded path, in path order, and reports each
/// write separately: a restore that half-succeeded says which half it is.
pub fn apply_patch_with(
    vcs: &dyn GitExecutor,
    root: &Path,
    patch: &Patch,
    rollback: &mut dyn Rollback,
) -> ApplyOutcome {
    let paths = touched_paths(patch);
    if let Err(reason) = startable(patch, &paths) {
        // Nothing has been read, written or dispatched.
        return ApplyOutcome::Aborted {
            reason,
            restored: Vec::new(),
        };
    }
    let recorded = match record(root, &paths) {
        Ok(r) => r,
        Err(reason) => {
            // Refused BEFORE the first write, which is the only moment a promise
            // of total rollback can still be made honestly.
            return ApplyOutcome::Aborted {
                reason,
                restored: Vec::new(),
            };
        }
    };

    for file in &patch.files {
        if let Err(e) = vcs.apply_patch_to_worktree(
            root,
            &Patch {
                files: vec![file.clone()],
            },
        ) {
            return roll_back(root, &recorded, rollback, e.to_string());
        }
    }
    ApplyOutcome::Applied { paths }
}

/// Put every recorded path back, and name it either as a total rollback or as
/// the failure of one.
///
/// Records hold the patch's own repo-relative path and restore at `root`'s
/// join of it, so the path the failure *names* is the one the developer read in
/// the patch rather than a machine-specific absolute path.
fn roll_back(
    root: &Path,
    recorded: &[Recorded],
    rollback: &mut dyn Rollback,
    reason: String,
) -> ApplyOutcome {
    let mut restored: Vec<PathBuf> = Vec::new();
    let mut unrestored: Vec<Unrestored> = Vec::new();
    for rec in recorded {
        match rollback.put(&root.join(&rec.path), rec.content.as_deref()) {
            Ok(()) => restored.push(rec.path.clone()),
            Err(e) => unrestored.push(Unrestored {
                path: rec.path.clone(),
                reason: e.to_string(),
                recorded: rec.content.clone(),
            }),
        }
    }
    if unrestored.is_empty() {
        ApplyOutcome::Aborted { reason, restored }
    } else {
        ApplyOutcome::RollbackFailed {
            reason,
            unrestored,
            restored,
        }
    }
}

/// The retry a `Rollback failed` state offers: write the recorded bytes again,
/// for exactly the paths that failed, and report the ones that fail again.
///
/// Deliberately not a git call and not a fresh apply — the content is already in
/// hand and the tree is already wrong, so the only honest second attempt is the
/// write that was refused. A path that has since disappeared records as restored
/// when its recorded state was absence, which is [`Worktree`]'s rule rather than
/// a second copy of it.
pub fn retry_restore(
    root: &Path,
    unrestored: &[Unrestored],
    rollback: &mut dyn Rollback,
) -> Vec<Unrestored> {
    let mut still = Vec::new();
    for rec in unrestored {
        if let Err(e) = rollback.put(&root.join(&rec.path), rec.recorded.as_deref()) {
            still.push(Unrestored {
                path: rec.path.clone(),
                reason: e.to_string(),
                recorded: rec.recorded.clone(),
            });
        }
    }
    still
}

/// The two conditions under which this module will not start at all: a patch
/// with no file sections (git would accept it silently and the surface would
/// report a success that did nothing), and a path it could not write back
/// inside `root` if it came to that.
///
/// The second is the load-bearing one. `git apply` guards its own paths, but the
/// **rollback** is this module's own write, and an absolute path or a `..`
/// component in a patch would hand it a destination outside the repository — so
/// an apply that could not be undone inside the root is refused rather than
/// attempted.
fn startable(patch: &Patch, paths: &[PathBuf]) -> Result<(), String> {
    if patch.files.is_empty() {
        return Err("the patch names no file".to_string());
    }
    if let Some(bad) = paths.iter().find(|p| !stays_inside(p)) {
        return Err(format!(
            "the patch names `{}`, which is outside the repository — a change this far \
             cannot be rolled back here, so nothing was applied",
            bad.display()
        ));
    }
    Ok(())
}

/// A repo-relative path with no absolute prefix and no parent step.
fn stays_inside(path: &Path) -> bool {
    path.is_relative()
        && !path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

/// Read every touched path out of the working tree.
///
/// A path that is not there records absence; a path that is there but cannot be
/// **read** is an error, not an absence. That distinction is the whole of the
/// record's value — treating an unreadable file as "absent" would have the
/// rollback delete a file it never saw, which is the failure mode this module is
/// named for.
fn record(root: &Path, paths: &[PathBuf]) -> Result<Vec<Recorded>, String> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        match std::fs::read(root.join(path)) {
            Ok(content) => out.push(Recorded {
                path: path.clone(),
                content: Some(content),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => out.push(Recorded {
                path: path.clone(),
                content: None,
            }),
            Err(e) => {
                return Err(format!(
                    "could not read `{}` to record its current contents ({e}) — with no \
                     record to restore from, nothing was applied",
                    path.display()
                ));
            }
        }
    }
    Ok(out)
}

/// The patch's own sections, for a surface that paints what was asked about.
pub fn files(patch: &Patch) -> &[PatchFile] {
    &patch.files
}

/// The error text the app's activity feed records when a `TgError` is all it has.
pub fn error_of(outcome: &ApplyOutcome) -> Option<TgError> {
    match outcome {
        ApplyOutcome::Applied { .. } => None,
        ApplyOutcome::Aborted { reason, .. } | ApplyOutcome::RollbackFailed { reason, .. } => {
            Some(TgError::Other(reason.clone()))
        }
    }
}
