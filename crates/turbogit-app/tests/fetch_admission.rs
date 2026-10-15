//! In-flight fetch admission — the one rule behind every fetch kind (ticket 02).
//!
//! The six per-kind guards are one owner now, so its contract is asserted once
//! here rather than six times: a second admit for a key already in flight is a
//! no-op, and a release — the settlement arm's one call, made before it matches
//! on the outcome — frees the slot for the next ask, Ok and Err alike.

use std::path::PathBuf;

use turbogit_app::fetch_admission::{FetchAdmission, InflightFetch};
use turbogit_app::root_caches::LogScope;
use turbogit_domain::model::RootId;

fn root(name: &str) -> RootId {
    RootId(PathBuf::from(format!("/repos/{name}")).into())
}

#[test]
fn one_fetch_in_flight_per_key_and_a_release_frees_the_slot() {
    let mut admission = FetchAdmission::default();
    let alpha = root("alpha");
    let log = InflightFetch::Log(alpha.clone());

    assert!(admission.admit(log.clone()), "a free key admits its fetch");
    assert!(
        !admission.admit(log.clone()),
        "one fetch in flight per key: a second admit for the same key is a no-op"
    );

    // The key is per fetch kind, not a bare root, so the same root's other
    // kinds take their own slots.
    assert!(
        admission.admit(InflightFetch::ScopedLog(
            alpha.clone(),
            LogScope::Ref("main".into())
        )),
        "a different fetch kind for the same root is a separate slot"
    );

    // The settlement arm releases before it matches on the outcome, so this one
    // call covers an Ok settlement …
    admission.release(log.clone());
    assert!(
        admission.admit(log.clone()),
        "an Ok settlement frees the slot"
    );
    // … and an Err settlement alike.
    admission.release(log.clone());
    assert!(
        admission.admit(log.clone()),
        "an Err settlement frees it too"
    );
}
