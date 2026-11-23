//! The polish suite, ported from the egui `r4_polish` at the reduced
//! fidelity the nsh ports keep: the shell holds at a small window, the
//! settings modal fits a short one, and every N6 dialog keeps its actions
//! inside its own frame. Focus-ring paint and stroke pixel probes are named
//! gaps in the N6 report; the headless engine renders no pixels.

use nsh_engine::Size;
use nsh_engine::geometry::Points;

use support::{Mounted, Project};

mod support;

const SMALL: (f32, f32) = (560.0, 420.0);
const SHORT: (f32, f32) = (900.0, 480.0);

fn shell(project: &Project) -> Mounted {
    let repo = support::seeded_repo(project.path(), "alpha");
    support::for_roots(project.path(), std::slice::from_ref(&repo))
}

fn resize(app: &mut Mounted, (w, h): (f32, f32)) {
    app.set_viewport(Size::new(Points(w), Points(h))).unwrap();
}

struct Bounds {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn bounds(app: &Mounted, id: &str) -> Bounds {
    let raw = id.strip_prefix("id:").unwrap_or(id);
    let node = app
        .screen()
        .layout_nodes()
        .into_iter()
        .find(|node| node.test_id.as_deref() == Some(raw))
        .unwrap_or_else(|| panic!("{id} paints a layout node"));
    Bounds {
        x: node.bounds.origin.x.0,
        y: node.bounds.origin.y.0,
        w: node.bounds.size.width.0,
        h: node.bounds.size.height.0,
    }
}

fn inside(inner: &Bounds, outer: &Bounds) -> bool {
    inner.x >= outer.x - 1.0
        && inner.y >= outer.y - 1.0
        && inner.x + inner.w <= outer.x + outer.w + 1.0
        && inner.y + inner.h <= outer.y + outer.h + 1.0
}

#[test]
fn the_shell_chrome_holds_at_a_small_window() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    resize(&mut app, SMALL);
    app.first_render().unwrap();

    for id in ["id:tab:Commit", "id:tab:Log", "id:toolbar:settings"] {
        let node = bounds(&app, id);
        assert!(
            node.x + node.w <= SMALL.0 + 1.0 && node.y + node.h <= SMALL.1 + 1.0,
            "{id} stays inside the 560x420 viewport: {node:?}",
            node = (node.x, node.y, node.w, node.h)
        );
    }
    let status = bounds(&app, "status-bar");
    assert!(
        status.y + status.h <= SMALL.1 + 1.0 && status.y >= SMALL.1 - 60.0,
        "the status bar pins to the bottom: {:?}",
        (status.x, status.y, status.w, status.h)
    );
}

#[test]
fn the_settings_modal_fits_a_short_window() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    resize(&mut app, SHORT);
    app.first_render().unwrap();
    app.press("id:toolbar:settings").unwrap();
    app.wait_for("id:dialog:settings").unwrap();

    let dialog = bounds(&app, "id:dialog:settings");
    assert!(
        dialog.y + dialog.h <= SHORT.1 + 1.0,
        "the settings card fits a 900x480 window: {:?}",
        (dialog.x, dialog.y, dialog.w, dialog.h)
    );
    for id in [
        "id:dialog:settings:action",
        "id:dialog:settings:cancel",
        "id:settings:restore-defaults",
    ] {
        let node = bounds(&app, id);
        assert!(
            node.y + node.h <= SHORT.1 + 1.0,
            "{id} stays on screen at 900x480"
        );
    }
}

#[test]
fn every_n6_dialog_keeps_its_actions_inside_its_frame() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();

    app.press("id:toolbar:settings").unwrap();
    app.wait_for("id:dialog:settings").unwrap();
    assert_dialog_actions(&app, "settings");
    app.press("id:dialog:settings:cancel").unwrap();
    app.wait_for_gone("id:dialog:settings", std::time::Duration::from_secs(5))
        .unwrap();

    app.press("id:toolbar:shortcuts").unwrap();
    app.wait_for("id:dialog:shortcuts").unwrap();
    assert_dialog_actions(&app, "shortcuts");
    app.press("id:dialog:shortcuts:cancel").unwrap();
    app.wait_for_gone("id:dialog:shortcuts", std::time::Duration::from_secs(5))
        .unwrap();

    app.press("id:toolbar:palette").unwrap();
    app.wait_for("id:dialog:command-palette").unwrap();
    assert_dialog_actions(&app, "command-palette");
    app.press("id:dialog:command-palette:cancel").unwrap();
    app.wait_for_gone(
        "id:dialog:command-palette",
        std::time::Duration::from_secs(5),
    )
    .unwrap();
}

fn assert_dialog_actions(app: &Mounted, name: &str) {
    let dialog = bounds(app, &format!("id:dialog:{name}"));
    let cancel = bounds(app, &format!("id:dialog:{name}:cancel"));
    assert!(
        inside(&cancel, &dialog),
        "the {name} dialog's dismiss action sits inside its frame: cancel {:?} vs dialog {:?}",
        (cancel.x, cancel.y, cancel.w, cancel.h),
        (dialog.x, dialog.y, dialog.w, dialog.h)
    );
    assert!(
        dialog.w >= 300.0 && dialog.h >= 80.0,
        "the {name} dialog is a dialog-sized float"
    );
}

#[test]
fn escape_closes_each_n6_dialog() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();

    for (open, dialog) in [
        ("id:toolbar:settings", "id:dialog:settings"),
        ("id:toolbar:shortcuts", "id:dialog:shortcuts"),
        ("id:toolbar:palette", "id:dialog:command-palette"),
    ] {
        app.press(open).unwrap();
        app.wait_for(dialog).unwrap();
        app.press_key("escape").unwrap();
        app.wait_for_gone(dialog, std::time::Duration::from_secs(5))
            .unwrap();
    }
}
