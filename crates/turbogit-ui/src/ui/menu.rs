//! The menu and the context menu host over nsh-base's Menu and the desktop
//! popup menu: the popover surface, the accent band on the row under the
//! pointer.

use nsh_base::{menu as base_menu, menu_item, menu_separator, theme};
use nsh_core::prelude::*;

use crate::theme as data;

/// One item of a menu.
#[derive(Debug, Clone)]
pub enum MenuEntry {
    /// A plain action, named.
    Item { label: SharedString, disabled: bool },
    /// A line between runs of items.
    Separator,
}

impl MenuEntry {
    pub fn item(label: impl Into<SharedString>) -> Self {
        MenuEntry::Item {
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self) -> Self {
        if let MenuEntry::Item {
            ref mut disabled, ..
        } = self
        {
            *disabled = true;
        }
        self
    }
}

/// The props of [`menu`].
pub struct MenuProps {
    /// The menu's items, in order.
    pub entries: Vec<MenuEntry>,
    /// The trigger: the control that opens the menu (already styled).
    pub trigger: AnyElement,
    /// The select handler, the chosen item's index into `entries`.
    pub on_select: Option<std::rc::Rc<dyn Fn(usize) + 'static>>,
    /// The id UI tests open the menu by: the trigger carries it.
    pub test_id: Option<SharedString>,
}

impl MenuProps {
    pub fn new(entries: Vec<MenuEntry>, trigger: impl IntoElement) -> Self {
        MenuProps {
            entries,
            trigger: trigger.into_element(),
            on_select: None,
            test_id: None,
        }
    }

    pub fn on_select(mut self, on_select: impl Fn(usize) + 'static) -> Self {
        self.on_select = Some(std::rc::Rc::new(on_select));
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A menu: the popover surface, the menu radius, the items' accent band.
/// A menu item element takes no test id, so a lane selects an item by text.
pub fn menu(props: MenuProps) -> impl IntoElement {
    let t = theme();
    let MenuProps {
        entries,
        trigger,
        on_select,
        test_id,
    } = props;
    let mut popup = base_menu()
        .popup_class("min-w-48 rounded-md p-1 shadow-lg bg-popover")
        .trigger(trigger);
    for (at, entry) in entries.into_iter().enumerate() {
        match entry {
            MenuEntry::Item { label, disabled } => {
                let mut item = menu_item()
                    .disabled(disabled)
                    .px_3()
                    .py_1()
                    .rounded(px(data::MARK_RADIUS))
                    .hover(|s| s.bg(t.accent))
                    .focus(|s| s.bg(t.accent))
                    .child(text(move || label.clone()).text_size(px(data::TYPE_CONTROL)));
                if let Some(on_select) = on_select.as_ref() {
                    let on_select = on_select.clone();
                    item = item.on_select(move || on_select(at));
                }
                popup = popup.child(item);
            }
            MenuEntry::Separator => {
                popup = popup.child(menu_separator().h(px(1.0)).my_1().bg(t.border));
            }
        }
    }
    match test_id {
        Some(id) => view().test_id(&id).child(popup),
        None => view().child(popup),
    }
}

/// A context menu host: the view that opens the desktop popup menu where a
/// secondary press lands. The host reports the press itself.
pub fn context_menu_host(
    host: View,
    entries: Vec<SharedString>,
    on_select: impl Fn(usize) + 'static,
) -> impl IntoElement {
    host.on_context_menu(move |event: &ContextMenuEvent| {
        let at = event.page_position;
        let items = entries
            .iter()
            .map(|label| nsh_core::element::desktop::MenuItem::new(label.to_string()))
            .collect::<Vec<_>>();
        spawn(async move {
            let _ = nsh_core::element::desktop::popup_menu_at(items, at).await;
        });
        on_select(entries.len());
    })
}
