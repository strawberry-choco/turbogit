//! The bridge test on the real engine: the pump drains a queued event into
//! the render, and a short borrow never crosses a callback.

fn headless() -> nsh_testing::Headless {
    test_support::nsh_harness::gallery().expect("the gallery mounts")
}

/// The gallery's bridge section queues an `AppEvent::Error` through the
/// handle; the pump drains it, the revision bumps, and the binding repaints
/// with the event's payload.
#[test]
fn the_pump_drains_a_queued_event_into_the_render() {
    let mut app = headless();
    app.first_render().unwrap();
    app.wait_for("text:last error: none").unwrap();

    app.press("id:demo:bridge-push").unwrap();
    app.wait_for("text:bridge event").unwrap();
}
