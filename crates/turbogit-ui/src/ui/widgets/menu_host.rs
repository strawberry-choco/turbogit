//! The shared context-menu host: one open/close lifecycle for every menu.
//!
//! A surface owns *which* menu is open — one field of its own state, so its
//! tests and its key ladder can both see it — and this module owns *how a menu
//! behaves once it is*: the pointer anchor, the floating surface, the
//! previous-frame trick that stops the right-click from also dismissing, the
//! click outside, Escape, and the close when the anchor has gone. It is told an
//! [`MenuId`] and given the contents to paint, so it knows nothing about
//! branches, commits or file rows, and a second menu in the window costs a
//! call site rather than a copy of the lifecycle.
//!
//! Two entry points, one per end of the gesture: [`note_anchor`] runs where the
//! row is painted and records the pointer, [`host_menu`] runs once per frame at
//! the end of the surface and paints what was opened.

use egui::{Pos2, Rect, Ui};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

use super::menu::menu_surface;

/// Which menu, and what it is open on. `salt` names the surface's menu and
/// `target` the thing it was opened on, so two menus — or one menu on two rows
/// — never share an anchor, a previous-frame tag, or an [`egui::Area`].
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

/// The identity key for one menu target: the row that opens a menu and the
/// surface that later paints it must agree on it, so it is built here rather
/// than restated at both ends. A root's FULL path, not its directory name, so
/// two repositories of the same name never share one menu's anchor.
pub fn target_of(root: &RootId, name: &str) -> String {
    format!("{}|{}", root.as_path().display(), name)
}

/// Record where a menu should open, at the moment of the right-click.
///
/// The anchor lives in egui memory rather than in the surface's state because
/// the row is painted by a component that has no application state to write it
/// to, and the surface keeps its own open state as the single thing its tests
/// and its key ladder read.
pub fn note_anchor(ui: &Ui, id: MenuId<'_>, pos: Pos2) {
    ui.ctx()
        .memory_mut(|m| m.data.insert_temp(id.anchor(), pos));
}

/// Paint one hosted menu's frame. Sets `dismiss` when the host decided the menu
/// is gone — Escape, a click outside after the menu had been visible for a
/// frame, or an anchor with no home — and returns what its contents answered
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
        // The anchor is gone: close rather than park a menu with no home.
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

    // Escape is the same one-press dismissal here for every surface that has no
    // key ladder of its own. `branches.rs` steps ahead of this in its own
    // ladder — the press belongs to the menu before it may touch the filter or
    // the selection — so for that surface the state is already cleared by the
    // time this runs, and the arm never fires.
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

/// The whole of "a row's context menu, once per frame": host it, close it if it
/// was dismissed, and close it again before running whatever it answered with.
///
/// **The unconditional close on a pick is the load-bearing half.** It is why `close`
/// runs on *both* paths rather than only inside the pick: without it the right-click
/// that just chose an item reopens the menu on the next frame, which no screenshot
/// covers.
///
/// `build` gets the read-only state because the row loop has just finished and the menu
/// needs to look a row up; the immutable borrow ends when `build` returns, which is why
/// `close` can take `&mut AppState` two lines later. `None` means "nothing was painted,
/// or the row is gone", which is not an error.
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
