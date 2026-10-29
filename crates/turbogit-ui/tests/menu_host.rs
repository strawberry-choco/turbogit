use egui::Ui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::harness::{assert_not_painted, assert_painted, right_click_row};
use test_support::srcscan;
use turbogit_ui::theme::{configure_style, install_fonts};
use turbogit_ui::ui::icons::Icon;

use turbogit_ui::ui::widgets::{self, MenuId, menu_host};

const ROW_A: &str = "alpha";
const ROW_B: &str = "beta";
const HOSTED_ITEM: &str = "Hosted item";

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
                state: widgets::MenuItemState::enabled(),
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

#[test]
fn a_right_click_opens_the_menu_at_the_pointer() {
    let mut harness = fixture_harness();
    harness.step();
    assert_not_painted(&harness, HOSTED_ITEM);

    right_click_row(&mut harness, ROW_A);

    assert_eq!(harness.state().open, Some(ROW_A));
    assert_painted(&harness, HOSTED_ITEM);
}

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

fn click_away_from_the_menu(harness: &mut Harness<'_, Fixture>) {
    let outside = egui::pos2(300.0, 360.0);
    harness.hover_at(outside);
    harness.step();
    harness.drag_at(outside);
    harness.step();
    harness.drop_at(outside);
    harness.step();
}

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

#[test]
fn a_menu_with_no_anchor_closes_instead_of_parking() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_A);

    harness.state_mut().open = Some(ROW_B);
    harness.step();

    assert_eq!(harness.state().open, None);
    assert_not_painted(&harness, HOSTED_ITEM);
}

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

#[test]
fn a_second_right_click_retargets_the_open_menu() {
    let mut harness = fixture_harness();
    right_click_row(&mut harness, ROW_B);
    assert_painted(&harness, HOSTED_ITEM);

    right_click_row(&mut harness, ROW_A);

    assert_eq!(harness.state().open, Some(ROW_A));
    assert_painted(&harness, HOSTED_ITEM);
}

#[test]
fn no_other_module_restates_the_menu_lifecycle() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let host = src.join("ui/widgets/menu_host.rs");
    let offenders: Vec<String> = srcscan::rust_files(&src)
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
