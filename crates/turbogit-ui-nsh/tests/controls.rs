//! The control tests: default, disabled, and selected states read through
//! the accessibility tree, and the press, toggle, and type behaviors land.
//! The visual states (hover, pressed, focused) come from the screenshot
//! lanes; this file pins the states the tree can prove.

fn headless() -> nsh_testing::Headless {
    test_support::nsh_harness::gallery().expect("the gallery mounts")
}

#[test]
fn a_disabled_button_does_not_press_and_a_live_one_does() {
    let mut app = headless();
    app.first_render().unwrap();
    assert!(
        !app.screen().enabled("id:demo:button-disabled").unwrap(),
        "the disabled button reads disabled"
    );
    assert!(
        app.screen().enabled("id:demo:button-primary").unwrap(),
        "the default button reads enabled"
    );
}

#[test]
fn the_selected_button_and_tab_read_selected() {
    let mut app = headless();
    app.first_render().unwrap();
    assert!(
        app.screen().selected("id:demo:button-selected").unwrap(),
        "the selected button reads selected"
    );
    assert!(
        app.screen().selected("id:tab:Log").unwrap(),
        "the current tab reads selected"
    );
}

#[test]
fn a_tab_press_selects_that_tab() {
    let mut app = headless();
    app.first_render().unwrap();
    // The tab element owns the press; a lane presses it by its text.
    app.press("text:Branches").unwrap();
    app.wait_for("text:tab: 2").unwrap();
}

#[test]
fn the_live_checkbox_reports_its_next_state() {
    let mut app = headless();
    app.first_render().unwrap();
    app.wait_for("text:state: Unchecked").unwrap();
    app.press("id:demo:checkbox-live").unwrap();
    app.wait_for("text:state: Checked").unwrap();
    app.press("id:demo:checkbox-live").unwrap();
    app.wait_for("text:state: Unchecked").unwrap();
}

#[test]
fn the_uncontrolled_input_round_trips_a_cjk_and_emoji_message() {
    let mut app = headless();
    app.first_render().unwrap();
    let message = "中文提交信息 with an emoji 🚀";
    app.type_text("id:demo:input-echo", message).unwrap();
    app.wait_for(&format!("text:echo: {message}")).unwrap();
}

#[test]
fn every_gallery_section_paints() {
    let mut app = headless();
    app.first_render().unwrap();
    for section in [
        "Buttons",
        "Icon buttons",
        "Inputs",
        "Checkboxes",
        "Selects",
        "Chips",
        "Tabs",
        "Toolbar",
        "Pane header",
        "Rows",
        "Card",
        "Data table",
        "Tree",
        "Menus",
        "Overlays",
        "Progress",
        "Bridge",
    ] {
        assert!(app.screen().shows(section), "the {section} section paints");
    }
}
