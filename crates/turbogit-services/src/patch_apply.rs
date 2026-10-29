use std::path::{Component, Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{Patch, PatchFile};
use turbogit_engine_api::GitExecutor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    /// Repo-relative, exactly as the patch section names it.
    pub path: PathBuf,
    pub content: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct Unrestored {
    pub path: PathBuf,
    pub reason: String,
    pub recorded: Option<Vec<u8>>,
}

impl Unrestored {
    pub fn stated(&self) -> String {
        format!("{} — {}", self.path.display(), self.reason)
    }
}

#[derive(Clone, Debug)]
pub enum ApplyOutcome {
    Applied {
        paths: Vec<PathBuf>,
    },
    Aborted {
        reason: String,
        restored: Vec<PathBuf>,
    },
    RollbackFailed {
        reason: String,
        unrestored: Vec<Unrestored>,
        restored: Vec<PathBuf>,
    },
}

impl ApplyOutcome {
    pub const ABORTED_TITLE: &'static str = "Apply patch aborted";
    pub const ROLLBACK_FAILED_TITLE: &'static str = "Rollback failed";
    pub const ABORTED_MESSAGE: &'static str =
        "All files changed during 'Apply Patch' action were rolled back";

    pub fn tree_is_restored(&self) -> bool {
        !matches!(self, Self::RollbackFailed { .. })
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Applied { .. } => "Patch applied",
            Self::Aborted { .. } => Self::ABORTED_TITLE,
            Self::RollbackFailed { .. } => Self::ROLLBACK_FAILED_TITLE,
        }
    }

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

pub trait Rollback {
    /// Put `content` back at `path`; `None` means the path must not exist.
    fn put(&mut self, path: &Path, content: Option<&[u8]>) -> TgResult<()>;
}

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
/// call mutates nothing, `git apply --check` writes no file and stages nothing.
///
/// The conflicting lines it reports are git's own stderr, verbatim, because that
/// text is the signal (the same rule [`GitExecutor::check_patch`] states for the
/// cherry-pick forecast); what this adds is the patch's path list, so a surface
/// can say which of the lines name a file the developer is about to touch.
#[derive(Clone, Debug)]
pub enum Preflight {
    Clean { files: usize },
    Conflicts { report: String, paths: Vec<PathBuf> },
    Refused { report: String },
}

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

pub fn apply_patch_with(
    vcs: &dyn GitExecutor,
    root: &Path,
    patch: &Patch,
    rollback: &mut dyn Rollback,
) -> ApplyOutcome {
    let paths = touched_paths(patch);
    if let Err(reason) = startable(patch, &paths) {
        return ApplyOutcome::Aborted {
            reason,
            restored: Vec::new(),
        };
    }
    let recorded = match record(root, &paths) {
        Ok(r) => r,
        Err(reason) => {
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
/// component in a patch would hand it a destination outside the repository, so
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

fn stays_inside(path: &Path) -> bool {
    path.is_relative()
        && !path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

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

pub fn files(patch: &Patch) -> &[PatchFile] {
    &patch.files
}

pub fn error_of(outcome: &ApplyOutcome) -> Option<TgError> {
    match outcome {
        ApplyOutcome::Applied { .. } => None,
        ApplyOutcome::Aborted { reason, .. } | ApplyOutcome::RollbackFailed { reason, .. } => {
            Some(TgError::Other(reason.clone()))
        }
    }
}
