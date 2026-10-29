use egui::{Pos2, Rect, Ui};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

use super::menu::menu_surface;

#[derive(Clone, Copy, Debug)]
pub struct MenuId<'a> {
    pub salt: &'static str,
    pub target: &'a str,
}

impl<'a> MenuId<'a> {
    pub fn new(salt: &'static str, target: &'a str) -> Self {
        Self { salt, target }
    }

    fn anchor(self) -> egui::Id {
        egui::Id::new(("tg_menu_anchor", self.salt, self.target))
    }

    fn painted_last_frame(self) -> egui::Id {
        egui::Id::new(("tg_menu_painted_last_frame", self.salt, self.target))
    }

    fn area(self) -> egui::Id {
        egui::Id::new(("tg_menu_area", self.salt, self.target))
    }
}

pub fn target_of(root: &RootId, name: &str) -> String {
    format!("{}|{}", root.as_path().display(), name)
}

pub fn note_anchor(ui: &Ui, id: MenuId<'_>, pos: Pos2) {
    ui.ctx()
        .memory_mut(|m| m.data.insert_temp(id.anchor(), pos));
}

/// Paint one hosted menu's frame. Sets `dismiss` when the host decided the menu
/// is gone, Escape, a click outside after the menu had been visible for a
/// frame, or an anchor with no home, and returns what its contents answered
/// with when anything was painted.
pub fn host_menu<T>(
    ui: &Ui,
    id: MenuId<'_>,
    open: bool,
    dismiss: &mut bool,
    contents: impl FnOnce(&mut Ui) -> T,
) -> Option<T> {
    let ctx = ui.ctx();
    if !open {
        ctx.memory_mut(|m| m.data.insert_temp(id.painted_last_frame(), false));
        return None;
    }
    let Some(pos) = ctx.memory(|m| m.data.get_temp::<Pos2>(id.anchor())) else {
        *dismiss = true;
        ctx.memory_mut(|m| m.data.insert_temp(id.painted_last_frame(), false));
        return None;
    };

    // A click outside only dismisses once the menu was visible on the PREVIOUS
    // frame, so the right-click that opened it cannot also close it (egui's own
    // dropdown idiom, `popups.rs`).
    let was_visible =
        ctx.memory(|m| m.data.get_temp::<bool>(id.painted_last_frame()) == Some(true));
    ctx.memory_mut(|m| m.data.insert_temp(id.painted_last_frame(), true));

    let ctx = ctx.clone();
    let mut painted = None;
    let mut area_rect = Rect::NOTHING;
    egui::Area::new(id.area()).fixed_pos(pos).show(&ctx, |ui| {
        menu_surface(ui).show(ui, |ui| {
            painted = Some(contents(ui));
        });
        area_rect = ui.min_rect();
    });

    if was_visible
        && ctx.input(|i| {
            i.key_pressed(egui::Key::Escape)
                || (i.pointer.any_click()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !area_rect.contains(p)))
        })
    {
        *dismiss = true;
    }
    painted
}

pub(crate) fn host_row_menu<T>(
    ui: &Ui,
    state: &mut AppState,
    salt: &'static str,
    target: String,
    build: impl FnOnce(&mut Ui, &AppState) -> Option<T>,
    close: fn(&mut AppState),
    apply: impl FnOnce(&mut AppState, T),
) {
    let mut dismiss = false;
    let read_only: &AppState = state;
    let picked = host_menu(ui, MenuId::new(salt, &target), true, &mut dismiss, |ui| {
        build(ui, read_only)
    });
    if dismiss {
        close(state);
    }
    if let Some(action) = picked.flatten() {
        close(state);
        apply(state, action);
    }
}
