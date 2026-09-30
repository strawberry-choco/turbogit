//! 3-way merge conflict model and resolution helpers.
//!
//! A merge conflict leaves up to three versions of a file in the index — the
//! common ancestor, "ours", and "theirs" — and the [`GitExecutor`] answers them
//! as one [`ConflictVersions`] value per path. This module reads that value and
//! offers helpers to stage a resolved file or to apply the simplest automatic
//! strategy.

use std::path::{Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// The three sides of one conflicted path, as the engine answers them.
pub fn read_versions(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
) -> TgResult<ConflictVersions> {
    vcs.conflict_versions(root, path)
}

/// Write `content` to the working tree copy and stage it as resolved.
pub fn write_resolution(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
    content: &str,
) -> TgResult<()> {
    std::fs::write(root.join(path), content)?;
    vcs.add(root, &[path.to_path_buf()])
}

/// The side a resolution takes, or the error saying the merge left no such side.
fn side(which: &str, chosen: &Option<String>) -> TgResult<String> {
    chosen.clone().ok_or_else(|| {
        TgError::Other(format!(
            "this conflict has no {which} side to accept — the other side deleted \
             the file, so resolve it as a deletion"
        ))
    })
}

/// Resolve a conflict by taking our side verbatim.
pub fn accept_ours(vcs: &dyn GitExecutor, root: &Path, path: &Path) -> TgResult<()> {
    let versions = read_versions(vcs, root, path)?;
    let ours = side("ours", &versions.ours)?;
    write_resolution(vcs, root, path, &ours)
}

/// Resolve a conflict by taking their side verbatim.
pub fn accept_theirs(vcs: &dyn GitExecutor, root: &Path, path: &Path) -> TgResult<()> {
    let versions = read_versions(vcs, root, path)?;
    let theirs = side("theirs", &versions.theirs)?;
    write_resolution(vcs, root, path, &theirs)
}

/// Try to resolve every conflicted file with the cheapest safe strategy.
///
/// A "simple" conflict is one where both sides made the identical change
/// (`ours == theirs`); in that case either side is correct, so we write one and
/// stage it. Paths that genuinely differ are left untouched (still reported as
/// `Ok` so the caller can iterate; the unresolved count is unchanged).
pub fn resolve_all_simple(
    vcs: &dyn GitExecutor,
    root: &Path,
    status: &RootStatus,
) -> Vec<(PathBuf, TgResult<()>)> {
    status
        .conflicted
        .iter()
        .map(|path| {
            let result = read_versions(vcs, root, path).and_then(|v| {
                if v.ours == v.theirs {
                    match v.ours {
                        Some(ours) => write_resolution(vcs, root, path, &ours),
                        // Both sides absent is not a change to write.
                        None => Ok(()),
                    }
                } else {
                    Ok(())
                }
            });
            (path.clone(), result)
        })
        .collect()
}

/// Number of conflicts still awaiting resolution.
pub fn unresolved(status: &RootStatus) -> usize {
    status.conflicted.len()
}

/// Parse a file's conflict markers into alternating normal / conflict blocks.
///
/// Returns `(segments, conflict_count)` where each segment is
/// `(ours, theirs, is_conflict)`; for normal segments `theirs` is empty.
/// This is the canonical marker parser for the merge editor — the UI's
/// fallback path when no base version is available for a structured 3-way
/// merge, and the reference the Phase L1 parity tests compare
/// [`turbogit_services::diff_engine::merge_segments`] against.
///
/// The three markers come from `turbogit_domain::model`, which is also where the two
/// surfaces that *paint* them read theirs, so the parser and both painters cannot name
/// different markers. A fourth — git's `|||||||` common-ancestor form, which this parser
/// does not handle — can only arrive by changing one place.
pub fn parse_conflict_markers(content: &str) -> (Vec<(String, String, bool)>, usize) {
    let mut segs: Vec<(String, String, bool)> = Vec::new();
    let mut conflicts = 0usize;
    let mut normal = String::new();
    let mut ours = String::new();
    let mut theirs = String::new();
    // mode: 0 = normal, 1 = inside ours, 2 = inside theirs
    let mut mode = 0u8;
    for line in content.lines() {
        if line.starts_with(CONFLICT_MARKER_OURS) {
            if !normal.is_empty() {
                segs.push((std::mem::take(&mut normal), String::new(), false));
            }
            mode = 1;
            conflicts += 1;
            ours.clear();
            theirs.clear();
        } else if line.starts_with(CONFLICT_MARKER_SEPARATOR) && mode == 1 {
            mode = 2;
        } else if line.starts_with(CONFLICT_MARKER_THEIRS) && (mode == 1 || mode == 2) {
            segs.push((std::mem::take(&mut ours), std::mem::take(&mut theirs), true));
            mode = 0;
        } else {
            match mode {
                0 => {
                    normal.push_str(line);
                    normal.push('\n');
                }
                1 => {
                    ours.push_str(line);
                    ours.push('\n');
                }
                _ => {
                    theirs.push_str(line);
                    theirs.push('\n');
                }
            }
        }
    }
    if !normal.is_empty() {
        segs.push((normal, String::new(), false));
    }
    (segs, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_engine::fake::FakeExecutor;

    #[test]
    fn write_resolution_writes_file_then_stages_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let engine = FakeExecutor::new();

        write_resolution(&engine, &repo, Path::new("src/main.rs"), "resolved!").unwrap();

        let written = std::fs::read_to_string(repo.join("src").join("main.rs")).unwrap();
        assert_eq!(written, "resolved!");
        let calls = engine.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "one stage call recorded");
    }

    #[test]
    fn parse_conflict_markers_splits_normal_and_conflict_blocks() {
        let content = "head\n<<<<<<< head\nours\n=======\ntheirs\n>>>>>>> tail\ntail\n";
        let (segs, n) = parse_conflict_markers(content);
        assert_eq!(n, 1);
        assert_eq!(
            segs,
            vec![
                ("head\n".to_owned(), String::new(), false),
                ("ours\n".to_owned(), "theirs\n".to_owned(), true),
                ("tail\n".to_owned(), String::new(), false),
            ]
        );
    }
}
