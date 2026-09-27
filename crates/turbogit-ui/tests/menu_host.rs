//! Contract tests for the shared context-menu host (`ui::widgets::menu_host`).
//!
//! One seam: the host itself, driven by a fixture surface that owns nothing but
//! "which row's menu is open". The open/close lifecycle is the whole contract —
//! where a menu appears and every way it can go away — so it is asserted here
//! once, for any menu in the app. That one host serving three surfaces is what
//! makes the branches suite passing untouched the evidence that the extraction
//! preserved behaviour rather than moved it.

use egui::Ui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::path::{Path, PathBuf};
use test_support::harness::{assert_not_painted, assert_painted};
use turbogit_ui::theme::{configure_style, install_fonts};
use turbogit_ui::ui::icons::Icon;

use turbogit_ui::ui::widgets::{self, MenuId, menu_host};

/// The two rows of the fixture surface, and the item its menu paints.
const ROW_A: &str = "alpha";
const ROW_B: &str = "beta";
const HOSTED_ITEM: &str = "Hosted item";

/// A surface with two right-clickable rows, an open state, and nothing else.
/// It owns which menu is open and what its item chose; the host is only ever
/// told the first of those.
#[derive(Default)]
struct Fixture {
    open: Option<&'static str>,
    acted: Option<&'static str>,
}

fn show_menu(ui: &Ui, row: &'static str, dismiss: &mut bool) -> Option<&'static str> {
    menu_host::host_menu(ui, MenuId::new("fixture", row), true, dismiss, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        widgets::menu_item(
            ui,
            widgets::MenuItemProps {
                icon: Icon::CHECK,
                label: HOSTED_ITEM,
                data: Some(row),
                shortcut: None,
                enabled: true,
                disabled_reason: None,
                kind: widgets::MenuItemKind::Default,
            },
        )
        .clicked()
        .then_some(row)
    })
    .flatten()
}

fn fixture_harness() -> Harness<'static, Fixture> {
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, fx| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                for row in [ROW_A, ROW_B] {
                    if ui.button(row).secondary_clicked() {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            menu_host::note_anchor(ui, MenuId::new("fixture", row), pos);
                        }
                        fx.open = Some(row);
                    }
                }
                let mut dismiss = false;
                if let Some(row) = fx.open
                    && let Some(acted) = show_menu(ui, row, &mut dismiss)
                {
                    fx.acted = Some(acted);
                }
                if dismiss {
                    fx.open = None;
                }
            });
        },
        Fixture::default(),
    );
    harness.set_size(egui::vec2(600.0, 400.0));
    harness
}

fn right_click_row(harness: &mut Harness<'_, Fixture>, row: &str) {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(row.to_string()))
        .unwrap_or_else(|| panic!("row {row}"))
        .click_secondary();
    harness.step();
    harness.step();
}

/// A right-click stashes the pointer as the menu's anchor, and the host floats
/// its contents there on the frame the row was clicked.
#[test]
fn a_right_click_opens_the_menu_at_the_pointer() {
    let mut harness = fixture_harness();
    harness.step();
    assert_not_painted(&harness, HOSTED_ITEM);

    right_click_row(&mut harness, ROW_A);

    assert_eq!(harness.state().open, Some(ROW_A));
    assert_painted(&harness, HOSTED_ITEM);
}

/// A click anywhere outside the menu closes it, and the next frame shows
/// nothing.
#[test]
fn a_click_outside_closes_the_menu() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_A);
    assert_painted(&harness, HOSTED_ITEM);

    click_away_from_the_menu(&mut harness);

    assert_eq!(harness.state().open, None, "the host dismissed it");
    harness.step();
    assert_not_painted(&harness, HOSTED_ITEM);
}

/// Press and release at a point the open menu does not cover.
fn click_away_from_the_menu(harness: &mut Harness<'_, Fixture>) {
    let outside = egui::pos2(300.0, 360.0);
    harness.hover_at(outside);
    harness.step();
    harness.drag_at(outside);
    harness.step();
    harness.drop_at(outside);
    harness.step();
}

/// Escape closes the open menu and nothing else: the surface's own state is the
/// only thing that changes.
#[test]
fn escape_closes_the_menu() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_A);
    assert_painted(&harness, HOSTED_ITEM);

    harness.key_press(egui::Key::Escape);
    harness.step();

    assert_eq!(harness.state().open, None);
    harness.step();
    assert_not_painted(&harness, HOSTED_ITEM);
}

/// A menu with no anchor cannot be painted, so it goes rather than parking with
/// no home: the surface is told to clear its own open state.
#[test]
fn a_menu_with_no_anchor_closes_instead_of_parking() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_A);

    // The surface claims ROW_B's menu is open; nothing ever anchored it.
    harness.state_mut().open = Some(ROW_B);
    harness.step();

    assert_eq!(harness.state().open, None);
    assert_not_painted(&harness, HOSTED_ITEM);
}

/// A click inside the menu is not a click outside it: the host keeps the menu
/// up and hands the surface what the contents answered with.
#[test]
fn clicking_an_item_reaches_the_surface_that_owns_the_action() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_A);

    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(HOSTED_ITEM.to_string()))
        .expect("the hosted item")
        .click();
    harness.step();

    assert_eq!(harness.state().acted, Some(ROW_A));
    assert_eq!(
        harness.state().open,
        Some(ROW_A),
        "closing an item's menu is the surface's decision, not the host's"
    );
}

/// Right-clicking a second row while a menu is open moves the menu to the new
/// target rather than dismissing it: the new target has no previous frame, so
/// the click that opened it cannot be the click that closes it.
#[test]
fn a_second_right_click_retargets_the_open_menu() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_B);
    assert_painted(&harness, HOSTED_ITEM);

    right_click_row(&mut harness, ROW_A);

    assert_eq!(harness.state().open, Some(ROW_A));
    assert_painted(&harness, HOSTED_ITEM);
}

// --- the extraction's own guard: one lifecycle, one home -------------------------

/// Walk a directory, yielding every `.rs` path under it.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// The lifecycle is one component's, so the shapes it is built from appear there
/// and nowhere else: a pointer anchor read out of egui memory, and the
/// previous-frame tag that decides whether a click outside may dismiss. A second
/// copy one file over is the failure ADR-0023 names, and it is what the log's
/// two menus would reintroduce.
#[test]
fn no_other_module_restates_the_menu_lifecycle() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let host = src.join("ui/widgets/menu_host.rs");
    let offenders: Vec<String> = rust_files(&src)
        .into_iter()
        .filter(|path| path != &host)
        .filter(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            text.contains("get_temp::<Pos2>") || text.contains("tg_menu_painted_last_frame")
        })
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        offenders.is_empty(),
        "the menu lifecycle is restated in {offenders:?}"
    );
}
