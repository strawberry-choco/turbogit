use std::path::PathBuf;

use turbogit_app::worktree_lifecycle::WorktreeLifecycle;
use turbogit_domain::model::RootId;

fn root(name: &str) -> RootId {
    RootId(PathBuf::from(format!("/repos/{name}")).into())
}

#[test]
fn admission_is_one_fetch_per_root_and_is_stamped_with_the_current_epoch() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let beta = root("beta");

    assert_eq!(
        policy.admit(&alpha),
        Some(0),
        "a fresh root admits at epoch 0"
    );
    assert_eq!(
        policy.admit(&alpha),
        None,
        "one fetch per root: while alpha's list is pending a second is refused"
    );
    assert_eq!(
        policy.admit(&beta),
        Some(0),
        "admission is per root, not global"
    );
    assert_eq!(policy.admit(&beta), None);
}

#[test]
fn settlement_is_accepted_only_while_its_stamp_is_current() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");

    assert_eq!(policy.admit(&alpha), Some(0));
    policy.on_mutation(&alpha);
    assert!(
        !policy.settle(&alpha, 0),
        "a list fetched before the mutation must not land over the fresh refetch"
    );
    assert_eq!(
        policy.admit(&alpha),
        Some(1),
        "the refetch is admitted at the bumped epoch"
    );
    assert!(policy.settle(&alpha, 1), "the current stamp is accepted");
}

#[test]
fn a_stale_settlement_still_releases_the_slot_it_holds() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");

    assert_eq!(policy.admit(&alpha), Some(0));
    policy.on_mutation(&alpha);
    assert!(!policy.settle(&alpha, 0), "the pre-mutation list is stale");

    assert_eq!(
        policy.admit(&alpha),
        Some(1),
        "the mutation's refetch is no longer blocked"
    );
}

#[test]
fn a_mutation_only_moves_the_affected_root() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let beta = root("beta");

    assert_eq!(policy.admit(&alpha), Some(0));
    assert_eq!(policy.admit(&beta), Some(0));
    policy.on_mutation(&alpha);

    assert!(
        !policy.settle(&alpha, 0),
        "alpha's outstanding list is stale"
    );
    assert!(
        policy.settle(&beta, 0),
        "a sibling root's stamp is untouched by alpha's mutation"
    );
}

#[test]
fn clear_bumps_every_in_flight_root_and_releases_their_slots() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let beta = root("beta");

    assert_eq!(policy.admit(&alpha), Some(0));
    assert_eq!(policy.admit(&beta), Some(0));
    policy.clear();

    assert_eq!(policy.admit(&alpha), Some(1));
    assert_eq!(policy.admit(&beta), Some(1));
    assert!(
        !policy.settle(&alpha, 0),
        "a pre-reset settlement is rejected after the reset"
    );
}

#[test]
fn clear_leaves_a_root_without_an_in_flight_fetch_alone() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let beta = root("beta");

    assert_eq!(policy.admit(&alpha), Some(0));
    assert!(
        policy.settle(&alpha, 0),
        "alpha's fetch finished before the reset"
    );
    assert_eq!(
        policy.admit(&beta),
        Some(0),
        "beta's fetch is still in flight"
    );

    policy.clear();

    assert_eq!(
        policy.admit(&beta),
        Some(1),
        "only roots that can still settle late need invalidating"
    );
    assert_eq!(
        policy.admit(&alpha),
        Some(0),
        "a root with nothing in flight keeps its epoch across the reset"
    );
}

#[test]
fn a_pre_clear_settlement_does_not_release_the_admission_that_replaced_it() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");

    assert_eq!(policy.admit(&alpha), Some(0));
    policy.clear();
    assert_eq!(policy.admit(&alpha), Some(1));

    assert!(!policy.settle(&alpha, 0), "a pre-reset stamp is stale");

    assert_eq!(
        policy.admit(&alpha),
        None,
        "the in-flight refill still owns the admission slot"
    );

    assert!(policy.settle(&alpha, 1));
    assert_eq!(policy.admit(&alpha), Some(1), "the slot is free again");
}

#[test]
fn probe_admission_is_per_row_and_deduped() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let wt1 = PathBuf::from("/repos/alpha/wt-1");
    let wt2 = PathBuf::from("/repos/alpha/wt-2");

    assert!(
        policy.admit_probe(&alpha, &wt1),
        "the first probe for a row is admitted"
    );
    assert!(
        !policy.admit_probe(&alpha, &wt1),
        "a repeat probe for the same row is deduped, not doubled"
    );
    assert!(
        policy.admit_probe(&alpha, &wt2),
        "a different row on the same root is still admitted"
    );
}

#[test]
fn probe_settlement_releases_only_its_own_slot() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let wt1 = PathBuf::from("/repos/alpha/wt-1");

    assert!(policy.admit_probe(&alpha, &wt1));
    assert!(
        policy.settle_dirty(&alpha, &wt1),
        "settling releases the slot it owns"
    );
    assert!(
        !policy.settle_dirty(&alpha, &wt1),
        "a second settle is a harmless no-op"
    );
    assert!(
        policy.admit_probe(&alpha, &wt1),
        "the slot is free again for a later probe"
    );
}

#[test]
fn probe_admission_is_scoped_per_root() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let beta = root("beta");
    let shared = PathBuf::from("/shared-path/wt");

    assert!(
        policy.admit_probe(&alpha, &shared),
        "alpha admits its probe"
    );
    assert!(
        policy.admit_probe(&beta, &shared),
        "a sibling root's probe for the same path is independent (root isolation)"
    );
    assert!(policy.settle_dirty(&alpha, &shared));
    assert!(
        !policy.admit_probe(&beta, &shared),
        "beta's probe slot is untouched by alpha's settlement (still in flight)"
    );
    assert!(
        policy.settle_dirty(&beta, &shared),
        "beta settles its own probe"
    );
}

#[test]
fn clear_drops_in_flight_probe_state() {
    let mut policy = WorktreeLifecycle::default();
    let alpha = root("alpha");
    let wt1 = PathBuf::from("/repos/alpha/wt-1");

    assert!(policy.admit_probe(&alpha, &wt1));
    policy.clear();
    assert!(
        !policy.settle_dirty(&alpha, &wt1),
        "clear released the in-flight probe slot; a late settle is a no-op"
    );
}
