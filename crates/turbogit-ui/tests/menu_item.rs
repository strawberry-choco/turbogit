//! Contract tests for the shared menu item primitive (`ui::widgets::menu`).
//!
//! Everything is asserted through painted output and accessibility nodes —
//! row geometry, token fills, ink, the data segment, the shortcut column,
//! the rule, and the surface frame — never through private frame
//! construction. The item takes plain data plus `&mut Ui` and returns a
//! `Response`; that is the whole seam.

use egui::{Color32, Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::harness::{filled_rects, painted_galleys, painted_paths, stroked_rects};
use turbogit_ui::theme::{GROUP_ROW_HEIGHT, MENU_RADIUS, Palette, configure_style, install_fonts};
use turbogit_ui::ui::icons::Icon;
use turbogit_ui::ui::widgets::{MenuItemKind, MenuItemProps, menu_item, menu_rule, menu_surface};

// --- harness -----------------------------------------------------------------

/// Render one `menu_item` call per frame inside a column of the given width,
/// and report the `Response` facts the surface would see.
fn item_harness_at(
    width: f32,
    props: fn() -> MenuItemProps<'static>,
    clicked: std::rc::Rc<std::cell::Cell<bool>>,
) -> Harness<'static, ()> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                let response = menu_item(ui, props());
                if response.clicked() {
                    clicked.set(true);
                }
            });
        },
        (),
    );
    harness.set_size(egui::vec2(width, 240.0));
    harness.step();
    harness
}

fn item_harness(
    props: fn() -> MenuItemProps<'static>,
    clicked: std::rc::Rc<std::cell::Cell<bool>>,
) -> Harness<'static, ()> {
    item_harness_at(320.0, props, clicked)
}

fn props(label: &'static str) -> MenuItemProps<'static> {
    MenuItemProps {
        icon: Icon::CHECK,
        label,
        data: None,
        shortcut: None,
        enabled: true,
        disabled_reason: None,
        kind: MenuItemKind::Default,
    }
}

/// The row rect of the item, read from its accessibility node.
fn item_rect(harness: &Harness<'_, ()>, label: &str) -> Rect {
    harness.get_by_label(label).rect()
}

// --- cycle 1: the row at rest --------------------------------------------------

/// A menu item is a 26 px row: an 8 px gutter to a 14 px glyph slot painted
/// in `INK_2`, then the label in the chrome face at `INK`.
#[test]
fn an_item_is_a_26px_row_with_glyph_slot_and_chrome_label() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let harness = item_harness(|| props("Checkout"), clicked);

    let row = item_rect(&harness, "Checkout");
    assert_eq!(
        row.height(),
        GROUP_ROW_HEIGHT,
        "the row is exactly one GROUP_ROW_HEIGHT tall"
    );

    // The glyph slot: 8 px of gutter, then a 14 px square the icon strokes
    // stay inside, painted in the icon ink.
    let slot = Rect::from_min_size(
        Pos2::new(row.left() + 8.0, row.center().y - 7.0),
        egui::vec2(14.0, 14.0),
    );
    let glyph = painted_paths(&harness)
        .into_iter()
        .find(|(rect, _color)| rect.min.x >= slot.min.x - 1.0 && rect.max.x <= slot.max.x + 1.0)
        .unwrap_or_else(|| panic!("no icon stroke painted inside the glyph slot {slot:?}"));
    assert_eq!(
        glyph.1,
        Palette::INK_2,
        "the glyph paints in the icon ink (INK_2)"
    );

    // The label: chrome face (Proportional), full INK, starting 8 px after
    // the glyph slot.
    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "Checkout")
        .expect("label painted");
    assert_eq!(
        label.family,
        egui::FontFamily::Proportional,
        "the label is chrome, not data"
    );
    assert_eq!(label.color, Palette::INK);
    assert!(
        (label.pos.x - (slot.right() + 8.0)).abs() < 1.0,
        "label starts 8 px after the glyph slot: {:?}",
        label.pos
    );
}

// --- cycle 2: hover / pressed fills -------------------------------------------

/// Every rect the last frame painted filled exactly `color`.
fn fills(harness: &Harness<'_, ()>, color: Color32) -> Vec<Rect> {
    filled_rects(harness)
        .into_iter()
        .filter(|(_, fill)| *fill == color)
        .map(|(rect, _)| rect)
        .collect()
}

/// Every painted `Shape::Rect`, descending into the `Shape::Vec` groups a
/// `Frame` paints (its shadow and border arrive as one compound shape).
fn flattened_rects(harness: &Harness<'_, ()>) -> Vec<(Rect, Color32, Color32)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(Rect, Color32, Color32)>) {
        match shape {
            egui::Shape::Rect(r) => {
                if r.fill != Color32::TRANSPARENT || r.stroke.width > 0.0 {
                    out.push((r.rect, r.stroke.color, r.fill));
                }
            }
            egui::Shape::Vec(group) => {
                for s in group {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &harness.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[track_caller]
fn assert_same_rect(a: Rect, b: Rect) {
    let close = |x: f32, y: f32| (x - y).abs() < 0.5;
    assert!(
        close(a.min.x, b.min.x)
            && close(a.min.y, b.min.y)
            && close(a.max.x, b.max.x)
            && close(a.max.y, b.max.y),
        "{a:?} != {b:?}"
    );
}

/// A resting row is transparent; hover fills `SURFACE_2` and a held press
/// fills `SURFACE_3` — the same row-state ladder every row in the app takes.
/// A completed press+release reports through the returned `Response`.
#[test]
fn a_row_fills_surface_2_on_hover_and_surface_3_while_pressed() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut harness = item_harness(|| props("Checkout"), clicked.clone());

    let row = item_rect(&harness, "Checkout");
    assert!(
        fills(&harness, Palette::SURFACE_2).is_empty(),
        "transparent at rest"
    );

    harness.hover_at(row.center());
    harness.step();
    let hover = fills(&harness, Palette::SURFACE_2);
    assert_eq!(hover.len(), 1, "hover paints exactly one row fill");
    assert_same_rect(hover[0], row);

    harness.drag_at(row.center());
    harness.step();
    let pressed = fills(&harness, Palette::SURFACE_3);
    assert_eq!(pressed.len(), 1, "a held press paints exactly one row fill");
    assert_same_rect(pressed[0], row);

    harness.drop_at(row.center());
    harness.step();
    assert!(
        clicked.get(),
        "the release lands as a click on the Response"
    );
}

// --- cycle 3: primary and danger kinds ------------------------------------------

/// The primary item carries the `KitButton::Primary` treatment the ⋯
/// overflow already gives Checkout: solid BRAND fill, BRAND_INK glyph and
/// label.
#[test]
fn a_primary_item_takes_the_brand_fill_and_ink() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let harness = item_harness(
        || MenuItemProps {
            kind: MenuItemKind::Primary,
            ..props("Checkout")
        },
        clicked,
    );

    let row = item_rect(&harness, "Checkout");
    let brand = fills(&harness, Palette::BRAND);
    assert_eq!(brand.len(), 1, "the primary row is one BRAND fill");
    assert_same_rect(brand[0], row);

    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "Checkout")
        .expect("label painted");
    assert_eq!(label.color, Palette::BRAND_INK, "label ink on brand");

    let slot = Rect::from_min_size(
        Pos2::new(row.left() + 8.0, row.center().y - 7.0),
        egui::vec2(14.0, 14.0),
    );
    let glyph = painted_paths(&harness)
        .into_iter()
        .find(|(rect, _)| rect.min.x >= slot.min.x - 1.0 && rect.max.x <= slot.max.x + 1.0)
        .expect("glyph painted");
    assert_eq!(glyph.1, Palette::BRAND_INK, "glyph ink on brand");
}

/// A danger item stays a plain row — no severity fill — and inks its glyph
/// and label with the `DANGER` token.
#[test]
fn a_danger_item_is_inked_with_the_severity_token_on_a_plain_row() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let harness = item_harness(
        || MenuItemProps {
            icon: Icon::TRASH_2,
            kind: MenuItemKind::Danger,
            ..props("Delete branch")
        },
        clicked,
    );

    assert!(
        fills(&harness, Palette::DANGER).is_empty(),
        "DANGER is ink, never a menu-row fill"
    );
    assert!(
        fills(&harness, Palette::BRAND).is_empty(),
        "a danger row is not primary"
    );
    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "Delete branch")
        .expect("label painted");
    assert_eq!(label.color, Palette::DANGER);
}

// --- cycle 4: the disabled row ---------------------------------------------------

/// A blocked item is never hidden: the row stays rendered with its ink at
/// 45 %, its click is swallowed, and the reason explains itself on hover —
/// the `delete_gate` / `rename_gate` convention of this codebase.
#[test]
fn a_disabled_item_stays_rendered_at_45_percent_ink_and_swallows_its_click() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut harness = item_harness(
        || MenuItemProps {
            enabled: false,
            disabled_reason: Some("no upstream to pull from"),
            ..props("Pull")
        },
        clicked.clone(),
    );

    // Still painted, at 45 % of full ink. Color32 stores premultiplied, so
    // the spec facts are the un-multiplied channels: INK's rgb, and an alpha
    // of 115 = round(255 × 0.45).
    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "Pull")
        .expect("a disabled item is never hidden");
    let [r, g, b, a] = label.color.to_srgba_unmultiplied();
    assert_eq!(a, 115, "disabled ink is INK dropped to 45 % alpha");
    let near = |x: u8, y: u8| (x as i32 - y as i32).abs() <= 2;
    assert!(
        near(r, 0xDF) && near(g, 0xE1) && near(b, 0xE5),
        "disabled ink keeps INK's hue, got {r:#04x}/{g:#04x}/{b:#04x}"
    );
    assert!(
        harness.get_by_label("Pull").accesskit_node().is_disabled(),
        "the accessibility node carries the disabled state"
    );

    // The click is swallowed.
    harness.get_by_label("Pull").click();
    harness.step();
    assert!(!clicked.get(), "a disabled row never reports a click");

    // The reason is reachable on hover.
    let row = item_rect(&harness, "Pull");
    harness.hover_at(row.center());
    let mut tooltip_painted = false;
    for _ in 0..20 {
        harness.step();
        if painted_galleys(&harness)
            .iter()
            .any(|g| g.text.contains("no upstream to pull from"))
        {
            tooltip_painted = true;
            break;
        }
    }
    assert!(
        tooltip_painted,
        "the disabled reason must appear as hover text"
    );
}

// --- cycle 5: data segment and shortcut column --------------------------------

/// The data segment paints in the data face after the prose label, and the
/// shortcut column sits right-aligned in muted mono — a name never reads as
/// a label, and an accelerator never collides with either.
#[test]
fn the_data_segment_paints_in_the_data_face_and_the_shortcut_hugs_the_right_edge() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let harness = item_harness_at(
        420.0,
        || MenuItemProps {
            label: "New branch from",
            data: Some("feature/cascade-views"),
            shortcut: Some("Ctrl+Shift+K"),
            ..props("New branch from")
        },
        clicked,
    );
    let row = item_rect(&harness, "New branch from");

    let galleys = painted_galleys(&harness);
    let label = galleys
        .iter()
        .find(|g| g.text == "New branch from")
        .expect("prose label painted");
    assert_eq!(label.family, egui::FontFamily::Proportional);
    let data = galleys
        .iter()
        .find(|g| g.text == "feature/cascade-views")
        .expect("data segment painted");
    assert_eq!(
        data.family,
        egui::FontFamily::Monospace,
        "a branch name never reads in the chrome face (design §19)"
    );
    assert!(
        data.pos.x >= label.rect.right(),
        "the data segment follows the label"
    );
    let shortcut = galleys
        .iter()
        .find(|g| g.text == "Ctrl+Shift+K")
        .expect("shortcut painted");
    assert_eq!(shortcut.family, egui::FontFamily::Monospace);
    assert_eq!(shortcut.color, Palette::T_MUTED);
    assert!(
        (shortcut.rect.right() - (row.right() - 8.0)).abs() < 1.0,
        "the shortcut column is right-aligned at the 8 px inset"
    );
}

/// Past the width where content no longer fits, the branch name
/// middle-truncates — the identifying ends stay, nothing wraps inside a row.
#[test]
fn a_long_branch_name_middle_truncates_in_the_data_segment() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let harness = item_harness(
        || MenuItemProps {
            label: "New branch from",
            data: Some("feature/a-very-long-branch-name-that-cannot-fit"),
            ..props("New branch from")
        },
        clicked,
    );

    let data = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text.contains('…'))
        .expect("the data segment middle-truncates");
    assert!(
        data.text.starts_with("feature/"),
        "the identifying prefix survives: {}",
        data.text
    );
    assert!(
        data.text.ends_with("ot-fit"),
        "the identifying suffix survives: {}",
        data.text
    );
}

/// A row too narrow for its label truncates the label; the shortcut column
/// never moves out of the right edge and the content never crosses it.
#[test]
fn a_narrow_row_truncates_the_label_before_the_shortcut_moves() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let harness = item_harness_at(
        260.0,
        || MenuItemProps {
            label: "New branch from a very long prose label",
            shortcut: Some("Ctrl+Shift+K"),
            ..props("New branch from a very long prose label")
        },
        clicked,
    );
    let row = item_rect(&harness, "New branch from a very long prose label");

    let galleys = painted_galleys(&harness);
    let shortcut = galleys
        .iter()
        .find(|g| g.text == "Ctrl+Shift+K")
        .expect("the shortcut never truncates");
    assert!(
        (shortcut.rect.right() - (row.right() - 8.0)).abs() < 1.0,
        "the shortcut column stays at the right edge"
    );
    let label = galleys
        .iter()
        .find(|g| g.text.contains('…'))
        .expect("the label middle-truncates");
    assert!(
        label.rect.right() <= shortcut.rect.min.x + 0.5,
        "truncated content stops before the shortcut column"
    );
}

// --- cycle 6: the rule, the surface, the focus ring -----------------------------

/// Render an arbitrary composition through the primitive's seam.
fn frame_harness(render: impl Fn(&mut egui::Ui) + 'static) -> Harness<'static, ()> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| render(ui));
        },
        (),
    );
    harness.set_size(egui::vec2(320.0, 240.0));
    harness
}

/// The rule is a real decision, not `ui.separator()`: a 1 px
/// `RULE_STRUCTURAL` (`LINE_SUBTLE`) hairline with 4 px of air either side.
/// `RULE_CONTENT` aliases the menu fill itself, so the structural tone is
/// the only one that reads inside a menu.
#[test]
fn the_menu_rule_paints_one_structural_hairline_with_air_either_side() {
    let mut harness = frame_harness(|ui| {
        // A menu composes its rows flush; the rule's stated air is the gap.
        ui.spacing_mut().item_spacing.y = 0.0;
        menu_item(ui, props("Alpha"));
        menu_rule(ui);
        menu_item(ui, props("Beta"));
    });
    harness.step();

    let row_a = item_rect(&harness, "Alpha");
    let row_b = item_rect(&harness, "Beta");
    let rules: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::LINE_SUBTLE)
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(rules.len(), 1, "exactly one rule rect painted");
    let rule = rules[0];
    assert!(rule.height() <= 1.0 + f32::EPSILON, "the rule is one pixel");
    assert_ne!(
        Palette::LINE_SUBTLE,
        Palette::SURFACE,
        "guard: the content-divider tone is the menu fill — invisible here"
    );
    assert!(
        (rule.min.y - row_a.max.y - 4.0).abs() < 0.5,
        "4 px of air below the previous item: {rule:?} / {row_a:?}"
    );
    assert!(
        (row_b.min.y - rule.max.y - 4.0).abs() < 0.5,
        "4 px of air above the next item: {rule:?} / {row_b:?}"
    );
}

/// The surface helper is the one place a menu inherits its tokens:
/// `Frame::popup(ui.style())` — SURFACE fill, LINE stroke, MENU_RADIUS —
/// so no call site restates them.
#[test]
fn the_menu_surface_frame_is_the_token_mapped_popup_frame() {
    let checked = std::rc::Rc::new(std::cell::Cell::new(false));
    let checked_ui = checked.clone();
    let mut harness = frame_harness(move |ui| {
        // The root Ui's style snapshot is only configured from the second
        // frame on (configure_style runs inside the render closure).
        if ui.style().visuals.window_fill() == Palette::SURFACE {
            let frame = menu_surface(ui);
            assert_eq!(frame.fill, Palette::SURFACE);
            assert_eq!(frame.stroke.color, Palette::LINE);
            assert_eq!(frame.stroke.width, 1.0);
            assert_eq!(
                frame.corner_radius,
                egui::CornerRadius::same(MENU_RADIUS),
                "the menu radius comes from configure_style"
            );
            checked_ui.set(true);
        }
        menu_surface(ui).show(ui, |ui| {
            menu_item(ui, props("Checkout"));
        });
    });
    harness.step();
    harness.step();
    assert!(
        checked.get(),
        "the configured popup style was never observed"
    );
    // And it really paints: `Frame::paint` emits its shadow + border as one
    // `Shape::Vec`, so the flattened frame rects carry the tokens.
    let frame_rects = flattened_rects(&harness);
    assert!(
        frame_rects
            .iter()
            .any(|(_, stroke, fill)| *stroke == Palette::LINE && *fill == Palette::SURFACE),
        "the frame paints SURFACE under a LINE border; got {frame_rects:?}"
    );
}

/// A keyboard-focused item takes the same brand focus ring as every other
/// custom-drawn control in the vocabulary.
#[test]
fn a_focused_item_paints_the_brand_focus_ring() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut harness = item_harness(|| props("Checkout"), clicked);
    let row = item_rect(&harness, "Checkout");
    assert!(
        stroked_rects(&harness)
            .iter()
            .all(|(_, color, _)| *color != Palette::BRAND),
        "no ring before focus"
    );

    harness.get_by_label("Checkout").focus();
    harness.step();
    let ring = stroked_rects(&harness)
        .into_iter()
        .find(|(rect, color, width)| {
            *color == Palette::BRAND && *width == 1.0 && (rect.height() - row.height()).abs() < 4.0
        })
        .expect("a 1 px BRAND focus ring around the row");
    assert!(
        (ring.0.min.x - (row.min.x - 1.0)).abs() < 1.0
            && (ring.0.max.x - (row.max.x + 1.0)).abs() < 1.0,
        "the ring sits one pixel outside the row: {:?}",
        ring.0
    );
}
