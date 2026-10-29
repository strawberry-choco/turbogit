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

    assert!(
        admission.admit(InflightFetch::ScopedLog(
            alpha.clone(),
            LogScope::Ref("main".into())
        )),
        "a different fetch kind for the same root is a separate slot"
    );

    admission.release(log.clone());
    assert!(
        admission.admit(log.clone()),
        "an Ok settlement frees the slot"
    );
    admission.release(log.clone());
    assert!(
        admission.admit(log.clone()),
        "an Err settlement frees it too"
    );
}

#[test]
fn an_incoming_poll_key_admits_once_and_frees_on_release() {
    let mut admission = FetchAdmission::default();
    let alpha = root("alpha");
    let poll = InflightFetch::IncomingPoll(alpha.clone());

    assert!(
        admission.admit(poll.clone()),
        "a free poll key admits its fetch"
    );
    assert!(
        !admission.admit(poll.clone()),
        "a root already polling is not admitted a second time"
    );

    assert!(
        admission.admit(InflightFetch::Log(alpha.clone())),
        "the same root's other fetch kind is a separate slot"
    );

    admission.release(poll.clone());
    assert!(
        admission.admit(poll.clone()),
        "the poll's settlement frees the slot"
    );
}
