use std::path::Path;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

pub fn log(vcs: &dyn GitExecutor, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
    vcs.log(root, opts)
}

pub fn blame(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
    rev: Option<&str>,
) -> TgResult<Vec<BlameLine>> {
    vcs.blame(root, path, rev)
}
