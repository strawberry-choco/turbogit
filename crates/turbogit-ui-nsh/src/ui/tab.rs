//! The tab list over nsh-base's Tabs: the shell's five tabs and any pane's
//! own tab row. The app owns which tab is current; a press mirrors it.

use nsh_base::{tab as base_tab, tabs as base_tabs, theme};
use nsh_core::prelude::*;

use crate::theme as data;

/// One tab of a list.
#[derive(Debug, Clone)]
pub struct TabItem {
    /// The value a press reports and the tab shows.
    pub label: SharedString,
    /// The id UI tests press it by. The tab element takes no test id, so
    /// the wrapper names the view that carries it.
    pub test_id: Option<SharedString>,
}

impl TabItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        TabItem {
            label: label.into(),
            test_id: None,
        }
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// The props of [`tabs`]. The current tab reads through a closure, so the
/// app's signal drives the ink, the indicator, and the selected state.
pub struct TabsProps {
    pub items: Vec<TabItem>,
    /// The current tab's index, read fresh on every render.
    pub selected: std::rc::Rc<dyn Fn() -> usize>,
    /// The press handler, the pressed tab's index.
    pub on_change: Option<std::rc::Rc<dyn Fn(usize) + 'static>>,
}

impl TabsProps {
    pub fn new(items: Vec<TabItem>, selected: impl Fn() -> usize + 'static) -> Self {
        TabsProps {
            items,
            selected: std::rc::Rc::new(selected),
            on_change: None,
        }
    }

    pub fn on_change(mut self, on_change: impl Fn(usize) + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(on_change));
        self
    }
}

/// A tab list: the muted band behind the tabs, the tab's ink steps up while
/// current, the indicator travels under the selected tab.
pub fn tabs(props: TabsProps) -> impl IntoElement {
    let t = theme();
    let on_change = props.on_change;
    let selected = props.selected;
    let items = props.items;
    base_tabs().indicator_class("inset-y-1 rounded-sm").child(
        view()
            .flex_row()
            .self_start()
            .gap_1()
            .p_1()
            .rounded(px(data::CONTROL_RADIUS))
            .bg(t.muted)
            .children(items.into_iter().enumerate().map(move |(at, item)| {
                let value = item.label.clone();
                // One clone per reader: each closure holds its own
                // handle for the tab's lifetime.
                let selected_for_tab = selected.clone();
                let selected_for_ink = selected.clone();
                let selected_for_a11y = selected.clone();
                let t = t.clone();
                let tab = base_tab()
                    .selected(move || selected_for_tab() == at)
                    .px_3()
                    .py_1()
                    .rounded(px(data::MARK_RADIUS))
                    .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
                    .hover(|s| s.opacity(0.85));
                let tab = tab.child(
                    text(move || value.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(move || {
                            if selected_for_ink() == at {
                                t.foreground
                            } else {
                                t.muted_foreground
                            }
                        }),
                );
                // The tab element carries the press (the press walk
                // stops at the first press-handling ancestor, which is
                // the tab, not the wrapper); the wrapper names it.
                let tab = match on_change.as_ref() {
                    Some(on_change) => {
                        let on_change = on_change.clone();
                        tab.on_press(move || on_change(at))
                    }
                    None => tab,
                };
                let mut host = view()
                    .flex_row()
                    .accessibility_state(move || AccessibilityState {
                        selected: selected_for_a11y() == at,
                        ..AccessibilityState::default()
                    });
                if let Some(id) = item.test_id.clone() {
                    // The press walk looks at the matched view and its
                    // ancestors, never its children: a test id on the
                    // wrapper only presses when the wrapper hears presses.
                    if let Some(on_change) = on_change.as_ref() {
                        let on_change = on_change.clone();
                        host = host.on_press(move || on_change(at));
                    }
                    host = host.test_id(&id);
                }
                host.child(tab)
            })),
    )
}
