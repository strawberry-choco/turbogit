//! The harness smoke test: the nsh harness mounts `TestRoot` over a
//! prepared state and the gallery paints; the repo fixture mounts too.

#[test]
fn the_harness_mounts_the_gallery() {
    let mut app = test_support::nsh_harness::gallery().expect("the gallery mounts");
    app.first_render().unwrap();
    assert!(
        app.screen().shows("Buttons"),
        "the gallery paints over a plain state"
    );
    assert!(
        app.screen().shows("Bridge"),
        "the bridge section paints: the state is mounted through the handle"
    );
}

#[test]
fn the_harness_mounts_a_prepared_repo() {
    let (_keep, repo) = test_support::nsh_harness::one_commit_repo();
    let mut app = test_support::nsh_harness::with_repo(&repo).expect("the repo mounts");
    app.first_render().unwrap();
    assert!(
        app.screen().shows("Buttons"),
        "the gallery paints over the prepared AppState::for_roots"
    );
}
