//! History service: log graph and blame. Thin orchestration over
//! [`GitExecutor`]; no git plumbing here.

use std::path::Path;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// Commit log for a root (log graph backing store).
pub fn log(vcs: &dyn GitExecutor, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
    vcs.log(root, opts)
}

/// Per-line blame for a file at an optional revision.
pub fn blame(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
    rev: Option<&str>,
) -> TgResult<Vec<BlameLine>> {
    vcs.blame(root, path, rev)
}
