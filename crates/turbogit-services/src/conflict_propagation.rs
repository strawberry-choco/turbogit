//! Cross-repo conflict propagation (issue #23, screen 07 "SAME CONFLICT
//! ELSEWHERE" panel).
//!
//! When the user resolves a conflict in one root, identical conflict hunks
//! in OTHER registered roots can be resolved the same way with one click.
//! This module owns the cross-repo side of that workflow:
//!
//! - [`find_matches`] — pure-read scan: for every other root, list the
//!   conflicted files whose conflict hunks (byte-identical `(ours, theirs)`
//!   pairs in the same order) match the source root's hunks. Returns one
//!   row per root that has at least one matching file.
//! - [`apply_to_matches`] — fan-out the resolution: for every matched
//!   root, write the source's composed resolution into each matched file
//!   and stage it. Per-root success/failure is reported; the source root is
//!   never touched, and roots with no matches are never touched.
//!
//! The service speaks git only through [`GitExecutor`] and the
//! [`turbogit_services::conflict`] helpers — no filesystem I/O outside the
//! engine seam. Refresh of the affected roots' status is the caller's job:
//! this module writes the resolved content and stages it, returning the
//! per-root outcomes so the caller can drive `rescan()` on the affected
//! `RootId`s.

use std::path::{Path, PathBuf};

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{MultiRootManager, RootId, RootStatus};
use turbogit_engine_api::GitExecutor;

use crate::conflict::{parse_conflict_markers, write_resolution};

/// One conflict-hunk resolution choice; matches the UI's
/// `conflict_resolver::CHOICE_*` constants exactly so a propagated
/// resolution can be replayed verbatim.
pub type Choice = u8;

/// Per-conflict resolution choices in hunk order. `Some(0)` = ours,
/// `Some(1)` = theirs, `Some(2)` = both (ours first), `Some(3)` = both
/// (theirs first). `None` = not yet resolved; propagation refuses to
/// replay an unresolved hunk (writes are gated on every hunk having a
/// choice).
pub type Resolutions = Vec<Option<Choice>>;

/// One conflicting hunk's `(ours, theirs)` signature, in file order.
/// Two files "match" iff their signatures are equal: same length, same
/// order, byte-identical `(ours, theirs)` pairs.
type HunkSignature = Vec<(String, String)>;

/// One file's signature extracted from its conflict markers.
fn signature_from(content: &str) -> HunkSignature {
    let (segs, _) = parse_conflict_markers(content);
    segs.into_iter()
        .filter_map(
            |(ours, theirs, is_conf)| {
                if is_conf { Some((ours, theirs)) } else { None }
            },
        )
        .collect()
}

/// One matched root: its id, the paths whose hunks matched the source's
/// hunks, and the number of matched hunks across those paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootMatch {
    pub root: RootId,
    pub paths: Vec<PathBuf>,
    pub matching_hunk_count: usize,
}

/// One root's propagation outcome: which path was rewritten, and whether
/// the rewrite + stage succeeded. Per-root failures never abort the
/// fan-out — every matched root gets exactly one outcome row.
#[derive(Clone, Debug)]
pub struct PropagateOutcome {
    pub root: RootId,
    pub path: PathBuf,
    pub result: TgResult<()>,
}

/// Locate, in every OTHER registered root, the conflicted files whose
/// conflict hunks are byte-identical to the source root's conflict hunks
/// for `source_path`. The source root itself is never scanned — the user
/// has already resolved it.
///
/// `source_resolutions` is read for its length (the expected hunk count);
/// `find_matches` itself does not mutate anything.
pub fn find_matches(
    executor: &dyn GitExecutor,
    mgr: &MultiRootManager,
    source_root: &RootId,
    source_path: &Path,
    source_resolutions: &Resolutions,
) -> Vec<RootMatch> {
    let source_signature = match read_signature(source_root, source_path) {
        Ok(sig) => sig,
        Err(_) => return Vec::new(),
    };
    // Sanity: the source resolution vector must cover every conflict hunk
    // in the source file. A mismatch means the caller passed stale state
    // — propagating it would silently drop resolutions, so refuse.
    if source_signature.len() != source_resolutions.len() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for root in &mgr.roots {
        if &root.id == source_root {
            continue;
        }
        let status = match executor.status(&root.id.0) {
            Ok(s) => s,
            Err(_) => continue, // a broken root shouldn't abort the scan
        };
        let matched_paths = match collect_matching_paths(&root.id.0, &status, &source_signature) {
            Some(p) => p,
            None => continue,
        };
        if matched_paths.is_empty() {
            continue;
        }
        let matching_hunk_count = source_signature.len() * matched_paths.len();
        out.push(RootMatch {
            root: root.id.clone(),
            paths: matched_paths,
            matching_hunk_count,
        });
    }
    out
}

/// Rewrite the source's resolution into every matched root's matched
/// files, staging each resolved file. Per-root failures are isolated: a
/// failure in one root never prevents the others from being attempted, and
/// the failure surfaces in the matching [`PropagateOutcome::result`].
///
/// Roots with no matching files are absent from the returned vector —
/// they were not touched in any way. Roots that the engine cannot read
/// DO appear, carrying an error [`PropagateOutcome::result`] for the
/// source path, so the caller can report per-root failure.
pub fn apply_to_matches(
    executor: &dyn GitExecutor,
    mgr: &MultiRootManager,
    source_root: &RootId,
    source_path: &Path,
    source_resolutions: &Resolutions,
) -> Vec<PropagateOutcome> {
    let mut outcomes = Vec::new();
    for root in &mgr.roots {
        if &root.id == source_root {
            continue;
        }
        let status = match executor.status(&root.id.0) {
            Ok(s) => s,
            Err(e) => {
                outcomes.push(PropagateOutcome {
                    root: root.id.clone(),
                    path: source_path.to_path_buf(),
                    result: Err(e),
                });
                continue;
            }
        };
        let source_signature = match read_signature(source_root, source_path) {
            Ok(s) => s,
            Err(e) => {
                outcomes.push(PropagateOutcome {
                    root: root.id.clone(),
                    path: source_path.to_path_buf(),
                    result: Err(e),
                });
                continue;
            }
        };
        let matched_paths = match collect_matching_paths(&root.id.0, &status, &source_signature) {
            Some(p) => p,
            None => continue,
        };
        for path in &matched_paths {
            let result = rewrite_and_stage(executor, &root.id.0, path, source_resolutions);
            outcomes.push(PropagateOutcome {
                root: root.id.clone(),
                path: path.clone(),
                result,
            });
        }
    }
    outcomes
}

/// Read `root/path`'s working-tree content and extract its conflict-hunk
/// signature. A read failure surfaces as a `TgError` so the caller can
/// decide whether to skip the root.
fn read_signature(root: &RootId, path: &Path) -> TgResult<HunkSignature> {
    let content = std::fs::read_to_string(root.0.join(path))
        .map_err(|e| TgError::Other(format!("read {}: {e}", path.display())))?;
    Ok(signature_from(&content))
}

/// Walk `root`'s conflicted paths and return those whose signature equals
/// `target_signature`. An empty vector means the root has no matching
/// files (and will be skipped by `find_matches`); `None` is reserved for
/// "can't tell" (currently unused — every conflict file is readable).
fn collect_matching_paths(
    root: &Path,
    status: &RootStatus,
    target_signature: &HunkSignature,
) -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    for path in &status.conflicted {
        let content = std::fs::read_to_string(root.join(path)).ok()?;
        let sig = signature_from(&content);
        if &sig == target_signature && !sig.is_empty() {
            out.push(path.clone());
        }
    }
    Some(out)
}

/// Rewrite `root/path` from its conflict markers using `resolutions` and
/// stage the result. The resolution vector is indexed by the source's
/// hunk order; the target's hunk order must match (which is what makes
/// the file a "match" in the first place).
fn rewrite_and_stage(
    executor: &dyn GitExecutor,
    root: &Path,
    path: &Path,
    resolutions: &Resolutions,
) -> TgResult<()> {
    let content = std::fs::read_to_string(root.join(path))
        .map_err(|e| TgError::Other(format!("read {}: {e}", path.display())))?;
    let composed = compose(&content, resolutions)?;
    write_resolution(executor, root, path, &composed)
}

/// Compose the resolved file text from `content`'s segments, applying
/// `resolutions` to each conflict hunk in order. Unresolved hunks
/// surface as `TgError` — a partial rewrite would leave the file in a
/// half-resolved state that the resolver UI later has to interpret, and
/// the propagation contract is "the same resolution everywhere, cleanly".
fn compose(content: &str, resolutions: &Resolutions) -> TgResult<String> {
    let (segs, conflict_count) = parse_conflict_markers(content);
    if conflict_count != resolutions.len() {
        return Err(TgError::Other(format!(
            "target has {conflict_count} conflict hunks but source had {} resolutions",
            resolutions.len(),
        )));
    }
    let mut out = String::new();
    let mut ci = 0usize;
    for (a, b, is_conf) in &segs {
        if *is_conf {
            let choice = match resolutions.get(ci).copied().flatten() {
                Some(c) => c,
                None => {
                    return Err(TgError::Other(format!(
                        "conflict hunk {ci} has no resolution; refusing to propagate partially"
                    )));
                }
            };
            ci += 1;
            match choice {
                0 => out.push_str(a), // ours
                1 => out.push_str(b), // theirs
                2 => {
                    out.push_str(a);
                    out.push_str(b);
                } // both, ours first
                3 => {
                    out.push_str(b);
                    out.push_str(a);
                } // both, theirs first
                other => {
                    return Err(TgError::Other(format!(
                        "unknown resolution choice {other} for hunk {ci}"
                    )));
                }
            }
        } else {
            out.push_str(a);
        }
    }
    Ok(out)
}
