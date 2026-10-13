//! The settlement module's routing table, as values over the two enums it
//! spans: `Operation` (what the Shell can dispatch) and `OpKind` (what
//! settlement routes on).
//!
//! Either half can drift from the other. A kind an operation produces that the
//! module never routes is a verb that settles on the generic path by accident;
//! a kind the module routes that nothing produces is a rule waiting for the
//! operation that will need it. So the two sets are compared as *values* here —
//! one operation per variant, through `Operation::kind()`, against the set the
//! module says it routes — rather than by reading either file.

use std::path::PathBuf;

use turbogit_app::OwnedOperation;
use turbogit_app::operation::{
    ApplyReport, LifecycleVerb, OpKind, Operation, PushRequest, RebaseKind, RecoveryMode,
    RecoveryPlan,
};
use turbogit_app::root_caches::Affected;
use turbogit_app::settle::ROUTED_KINDS;
use turbogit_domain::model::{CleanTreeMethod, MergeOpts, Patch, RebaseOpts, RootId, VcsSettings};

/// A full commit id, as the log carries it.
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn root(name: &str) -> RootId {
    RootId(PathBuf::from("/repo").join(name).into())
}

fn settings() -> VcsSettings {
    VcsSettings::default()
}

fn push_request() -> PushRequest {
    PushRequest::single(
        root("alpha"),
        "origin".to_string(),
        "feature".to_string(),
        &settings(),
    )
}

/// One operation per [`Operation`] variant — the exhaustive set of what the
/// Shell can actually dispatch. `Push` appears twice because the recovery plan
/// is what makes the same variant two identities.
fn one_of_every_variant() -> Vec<Operation> {
    let r = root("alpha");
    let s = settings();
    vec![
        Operation::Fetch {
            roots: vec![r.clone()],
        },
        Operation::Merge {
            root: r.clone(),
            target: "topic".into(),
            opts: MergeOpts::default(),
            clean: CleanTreeMethod::Stash,
            commits: 0,
        },
        Operation::Rebase {
            root: r.clone(),
            branch: "feature".into(),
            settings: s.clone(),
            opts: RebaseOpts::default(),
            kind: RebaseKind::Standard {
                onto: "main".into(),
            },
        },
        Operation::DeleteBranch {
            root: r.clone(),
            name: "spare".into(),
        },
        Operation::shelve(r.clone(), Vec::new(), "note"),
        Operation::Unshelve {
            root: r.clone(),
            name: "shelf".into(),
            remove: false,
            patch: None,
        },
        Operation::WorktreeAdd {
            root: r.clone(),
            path: PathBuf::from("/repo/wt"),
            branch: "wt-branch".into(),
        },
        Operation::WorktreeRemove {
            root: r.clone(),
            path: PathBuf::from("/repo/wt"),
        },
        Operation::drop_commit(&r, "feature", SHA, &s),
        Operation::reword_commit(&r, "feature", SHA, "a better message", &s),
        Operation::push(push_request()),
        Operation::push_with_recovery(
            push_request(),
            RecoveryPlan {
                mode: RecoveryMode::Fetch,
                roots: vec![r.clone()],
            },
        ),
        Operation::Resolve {
            root: r.clone(),
            owner: OwnedOperation::Merge,
            verb: LifecycleVerb::Continue,
        },
        Operation::GetFileFromBranch {
            root: r.clone(),
            source: "feature".into(),
            path: PathBuf::from("/repo/notes.txt"),
        },
        Operation::ApplyPatch {
            root: r.clone(),
            patch: Patch::default(),
            report: ApplyReport::armed(),
        },
        Operation::custom("Anything", Affected::Root(r), |_| Ok(())),
    ]
}

fn sorted(kinds: impl IntoIterator<Item = OpKind>) -> Vec<OpKind> {
    let mut kinds: Vec<OpKind> = kinds.into_iter().collect();
    kinds.sort_by_key(|kind| format!("{kind:?}"));
    kinds
}

/// The two sets are the same set. A kind added to `OpKind` without a row in
/// the module's routing table is a compile error at its total match *and* a
/// failure here; an `Operation` variant that settles under an identity nobody
/// routes is only caught here.
#[test]
fn every_kind_an_operation_produces_is_one_settlement_routes() {
    let produced = sorted(one_of_every_variant().iter().map(Operation::kind));
    let routed = sorted(ROUTED_KINDS.iter().copied());

    // Uniqueness first, so a duplicate names the side that has one instead of
    // arriving as a diff between two lists that are otherwise the same set.
    let mut deduped = produced.clone();
    deduped.dedup();
    assert_eq!(
        deduped.len(),
        produced.len(),
        "two operations settled under one identity, so the table above is missing a row"
    );
    let mut routed_once = routed.clone();
    routed_once.dedup();
    assert_eq!(
        routed_once.len(),
        routed.len(),
        "the module's routing table names a kind twice"
    );
    assert_eq!(
        routed, produced,
        "settlement routes a different set of kinds than the Shell can dispatch"
    );
}
