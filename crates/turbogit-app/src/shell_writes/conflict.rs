use std::path::Path;

use turbogit_domain::model::{RootId, RootStatus};
use turbogit_services::conflict;

use crate::operation::Operation;
use crate::root_caches::Affected;

pub fn accept_ours(root: &RootId, path: &Path) -> Operation {
    let r = root.clone();
    let p = path.to_path_buf();
    Operation::custom("Accept ours", Affected::Root(r.clone()), move |v| {
        conflict::accept_ours(v, r.as_path(), &p)
    })
}

pub fn accept_theirs(root: &RootId, path: &Path) -> Operation {
    let r = root.clone();
    let p = path.to_path_buf();
    Operation::custom("Accept theirs", Affected::Root(r.clone()), move |v| {
        conflict::accept_theirs(v, r.as_path(), &p)
    })
}

/// Stage every conflicted file both sides already agree on, and report the
/// first one that could not be written rather than swallowing it.
/// `status` is the root's own snapshot rather than a re-read, because it is the
/// set of paths this pass was promised to cover.
pub fn resolve_simple(root: &RootId, status: &RootStatus) -> Operation {
    let r = root.clone();
    let status = status.clone();
    Operation::custom(
        "Resolve simple conflicts",
        Affected::Root(r.clone()),
        move |v| {
            let results = conflict::resolve_all_simple(v, r.as_path(), &status);
            match results.into_iter().find_map(|(_, res)| res.err()) {
                Some(e) => Err(e),
                None => Ok(()),
            }
        },
    )
}

pub fn apply_merge_resolution(root: &RootId, path: &Path, content: &str) -> Operation {
    let r = root.clone();
    let p = path.to_path_buf();
    let content = content.to_string();
    Operation::custom(
        "Apply merge resolution",
        Affected::Root(r.clone()),
        move |v| conflict::write_resolution(v, r.as_path(), &p, &content),
    )
}
