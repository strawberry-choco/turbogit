//! The density lane's behavioral half: at a 200 percent font scale no
//! element leaves the viewport horizontally. The PNG half of the lane runs
//! only on a live window (the headless engine renders no pixels); the PR
//! names that gap.

use nsh_engine::geometry::Points;

fn headless() -> nsh_testing::Headless {
    test_support::nsh_harness::gallery().expect("the gallery mounts")
}

/// The viewport width the harness mounts at, in points.
const VIEWPORT_W: f32 = 1280.0;

#[test]
fn at_the_default_scale_no_element_leaves_the_viewport() {
    let mut app = headless();
    app.first_render().unwrap();
    for node in app.screen().layout_nodes() {
        if node.hidden || node.bounds.size.width.0 <= 0.0 {
            continue;
        }
        assert!(
            node.bounds.origin.x.0 >= -1.0,
            "{:?} starts left of the viewport at {:?}",
            node.test_id,
            node.bounds
        );
    }
}

#[test]
fn at_a_200_percent_font_scale_no_element_leaves_the_viewport() {
    let mut app = headless();
    app.first_render().unwrap();
    app.set_environment("font scale 200%", |env| env.font_scale = 2.0)
        .unwrap();
    app.first_render().unwrap();
    let mut widest = (None, 0.0_f32);
    for node in app.screen().layout_nodes() {
        if node.hidden || node.bounds.size.width.0 <= 0.0 {
            continue;
        }
        let right = node.bounds.origin.x.0 + node.bounds.size.width.0;
        if right > widest.1 {
            widest = (node.test_id.clone(), right);
        }
        assert!(
            right <= VIEWPORT_W + 1.0,
            "{:?} reaches {right} at the 200 percent scale: no control clips \
             horizontally",
            node.test_id
        );
    }
    let _ = Points(widest.1);
}
