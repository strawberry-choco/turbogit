//! The shared tab view widget (`src/ui/widgets/tabs.rs`).
//!
//! The component is app-free, so this suite renders it over a local enum and a
//! plain context struct — the contract under test is the widget's, not the
//! shell's. Assertions read painted output (galleys, filled rects) and the
//! harness's accessibility tree, the way the shell strip's own suite
//! (`workspace_shell_frame.rs`) pins the same marks at full-app scale:
//!
//! - the bar is the top `TAB_STRIP_HEIGHT` band, the active tab its one
//!   `BRAND` underline;
//! - the content sits `PANEL_PADDING` inside the claimed region, beneath the bar;
//! - a click reaches `on_select` with the right id and switches the body
//!   same-frame (content reads the caller's fresh state);
//! - the end section renders in the bar and its control is clickable.

use egui::{Ui, WidgetInfo, WidgetType};
use egui_kittest::{Harness, kittest::Queryable as _};
use test_support::harness::{filled_rects, painted_galleys, settle};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::icons::Icon;
use turbogit_ui::ui::widgets::{TAB_ITEM_HEIGHT, TAB_STRIP_HEIGHT, TabItem, tab_view};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    One,
    Two,
}

/// The app-free context: the selection the caller owns, the observations the
/// closures record, and the region the caller saw the widget claim.
struct Ctx {
    selected: Tab,
    clicks: Vec<Tab>,
    end_clicked: bool,
    region: Option<egui::Rect>,
}

fn tabs() -> [TabItem<'static, Tab>; 2] {
    [
        TabItem {
            id: Tab::One,
            title: "One",
            icon: Some(Icon::GIT_COMMIT),
            end_icon: None,
            badge: None,
        },
        TabItem {
            id: Tab::Two,
            title: "Two",
            icon: None,
            end_icon: None,
            badge: None,
        },
    ]
}

fn render(ui: &mut Ui, ctx: &mut Ctx) {
    let tabs = tabs();
    let active = ctx.selected;
    ctx.region = Some(ui.max_rect());
    tab_view(
        ui,
        ctx,
        &tabs,
        active,
        |ctx, tab| {
            ctx.clicks.push(tab);
            ctx.selected = tab;
        },
        |ui, ctx| {
            let end = turbogit_ui::ui::widgets::icon_button(ui, Icon::SETTINGS);
            end.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "end action"));
            if end.clicked() {
                ctx.end_clicked = true;
            }
        },
        |ui, ctx| {
            let body = match ctx.selected {
                Tab::One => "body one",
                Tab::Two => "body two",
            };
            ui.label(body);
        },
    );
}

fn harness(ctx: Ctx) -> Harness<'static, Ctx> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, ctx: &mut Ctx| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| render(ui, ctx));
        },
        ctx,
    );
    harness.set_size(egui::vec2(640.0, 400.0));
    settle(&mut harness);
    harness
}

fn fresh_ctx() -> Ctx {
    Ctx {
        selected: Tab::One,
        clicks: vec![],
        end_clicked: false,
        region: None,
    }
}

/// The tab label's painted rect, located by its string: one tab paints "One"
/// and the content body never does.
fn label(harness: &Harness<'_, Ctx>, text: &str) -> egui::Rect {
    let hits: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text)
        .collect();
    assert_eq!(hits.len(), 1, "one `{text}` painted, found {hits:#?}");
    hits.into_iter().next().expect("one hit").rect
}

/// The frame's BRAND-filled rects — the tab underlines, and nothing else this
/// fixture paints.
fn brand_fills(harness: &Harness<'_, Ctx>) -> Vec<egui::Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::BRAND)
        .map(|(rect, _)| rect)
        .collect()
}

/// The bar is the region's top `TAB_STRIP_HEIGHT` band: the active tab's
/// underline rides the band's bottom edge, its label centred in a
/// `TAB_ITEM_HEIGHT` item at the band's top.
#[test]
fn the_bar_is_the_top_strip_height_band() {
    let h = harness(fresh_ctx());
    let region = h.state().region.expect("the caller recorded its region");

    let underlines = brand_fills(&h);
    assert_eq!(
        underlines.len(),
        1,
        "exactly the active tab carries the accent underline: {underlines:?}"
    );
    let rule = underlines[0];

    // The label is vertically centred in its `TAB_ITEM_HEIGHT` item, and the
    // item starts at the region's top — so the painted label's centre yields
    // the band's top edge, and the band's bottom edge is one full strip below.
    let one = label(&h, "One");
    let top = one.center().y - TAB_ITEM_HEIGHT / 2.0;
    assert!(
        (top - region.top()).abs() <= 0.5,
        "the bar starts at the region's top edge: label {one:?}, region {region:?}"
    );
    assert!(
        (rule.bottom() - (top + TAB_STRIP_HEIGHT)).abs() <= 0.5,
        "the underline rides the strip's bottom edge: rule {rule:?}, band top {top}"
    );
    assert!(
        (rule.height() - turbogit_ui::theme::RAIL_WIDTH).abs() <= 0.5,
        "the underline is the token layer's accent width: rule {rule:?}"
    );
}

/// The content sits `PANEL_PADDING` inside the claimed region on the sides the
/// frame can see: one strip height plus one padding below the top edge, one
/// padding in from the left.
#[test]
fn the_content_sits_panel_padding_inside_the_region() {
    let h = harness(fresh_ctx());
    let region = h.state().region.expect("the caller recorded its region");

    let body = label(&h, "body one");
    assert!(
        (body.top() - (region.top() + TAB_STRIP_HEIGHT + turbogit_ui::theme::PANEL_PADDING)).abs()
            <= 0.5,
        "the content starts one strip plus one padding below the region's top: \
         body {body:?}, region {region:?}"
    );
    assert!(
        (body.left() - (region.left() + turbogit_ui::theme::PANEL_PADDING)).abs() <= 0.5,
        "the content is one padding in from the region's left edge: \
         body {body:?}, region {region:?}"
    );
}

/// A click calls `on_select` with the clicked id and switches the body on the
/// same frame: `on_select` runs before the content closure, which reads the
/// caller's state fresh.
#[test]
fn a_click_selects_the_tab_and_switches_the_body_same_frame() {
    let mut h = harness(fresh_ctx());

    h.get_by_label("Two").click();
    h.step();

    let state = h.state();
    assert_eq!(
        state.clicks,
        vec![Tab::Two],
        "the click reaches on_select with the clicked tab's id"
    );
    assert_eq!(state.selected, Tab::Two, "on_select's assignment lands");
    let _ = label(&h, "body two");
    let gone: Vec<_> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text == "body one")
        .collect();
    assert!(
        gone.is_empty(),
        "the previous body is gone the same frame the click lands: {gone:#?}"
    );
}

/// The end section renders in the bar and its control works.
#[test]
fn the_end_section_renders_in_the_bar_and_clicks() {
    let mut h = harness(fresh_ctx());
    let region = h.state().region.expect("the caller recorded its region");

    let end = h.get_by_label("end action").rect();
    assert!(
        end.top() >= region.top() && end.bottom() <= region.top() + TAB_STRIP_HEIGHT,
        "the end control lives inside the strip band: control {end:?}, region \
         {region:?}, strip height {TAB_STRIP_HEIGHT}"
    );
    let two = label(&h, "Two");
    assert!(
        end.left() > two.right(),
        "the end control sits right of the tabs: control {end:?}, last tab label {two:?}"
    );
    assert!(
        end.right() <= region.right(),
        "the end control stays inside the claimed region: control {end:?}, region \
         {region:?}"
    );

    h.get_by_label("end action").click();
    h.step();
    assert!(
        h.state().end_clicked,
        "the end section's control is clickable"
    );
}
