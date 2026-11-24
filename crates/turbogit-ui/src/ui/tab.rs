//! The tab list: the shell's five primary tabs (an underline rail) and a
//! pane's own secondary row (a segmented track). The app owns which tab is
//! current; a press mirrors it.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// One tab of a list.
#[derive(Debug, Clone)]
pub struct TabItem {
    /// The value a press reports and the tab shows.
    pub label: SharedString,
    /// The id UI tests press it by.
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

/// How a tab list paints: the shell's top-level rail, or a pane's own
/// segmented switch. Two roles, two shapes; neither is a third kind of
/// button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabsVariant {
    /// A quiet track with a raised current tab. A pane's sub-navigation.
    #[default]
    Segmented,
    /// Flat labels over a hairline, the current one marked by a brand rule.
    /// The window's primary navigation.
    Underline,
}

/// The props of [`tabs`]. The current tab reads through a closure, so the
/// app's signal drives the ink and the marker.
pub struct TabsProps {
    pub items: Vec<TabItem>,
    /// The current tab's index, read fresh on every render.
    pub selected: std::rc::Rc<dyn Fn() -> usize>,
    /// The press handler, the pressed tab's index.
    pub on_change: Option<std::rc::Rc<dyn Fn(usize) + 'static>>,
    pub variant: TabsVariant,
}

impl TabsProps {
    pub fn new(items: Vec<TabItem>, selected: impl Fn() -> usize + 'static) -> Self {
        TabsProps {
            items,
            selected: std::rc::Rc::new(selected),
            on_change: None,
            variant: TabsVariant::Segmented,
        }
    }

    pub fn variant(mut self, variant: TabsVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn on_change(mut self, on_change: impl Fn(usize) + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(on_change));
        self
    }
}

/// A tab list. The track (segmented) or the hairline (underline) belongs to
/// the list; the current marker belongs to the tab.
pub fn tabs(props: TabsProps) -> impl IntoElement {
    let t = theme();
    let variant = props.variant;
    let on_change = props.on_change;
    let selected = props.selected;
    let items = props.items;

    let list = match variant {
        TabsVariant::Segmented => view()
            .flex_row()
            .flex_wrap()
            .self_start()
            .gap(px(2.0))
            .p(px(2.0))
            .rounded(px(data::MENU_RADIUS))
            .bg(t.muted),
        TabsVariant::Underline => view().flex_row().flex_wrap().gap(px(2.0)),
    };

    list.children(items.into_iter().enumerate().map(move |(at, item)| {
        let value = item.label.clone();
        let current_for_ink = selected.clone();
        let current_for_state = selected.clone();
        let current_for_tab = selected.clone();
        let t = t.clone();
        let tab = view()
            .role(AccessibilityRole::Button)
            .flex_row()
            .items_center()
            .gap(px(data::SPACING_X))
            .rounded(px(match variant {
                TabsVariant::Segmented => data::CONTROL_RADIUS,
                TabsVariant::Underline => data::MARK_RADIUS,
            }))
            .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring));
        let tab = match variant {
            TabsVariant::Segmented => {
                let t_fill = t.clone();
                let t_hover = t.clone();
                let fill_state = current_for_tab.clone();
                let hover_state = current_for_tab.clone();
                tab.px(px(9.0))
                    .py(px(4.0))
                    .bg(move || {
                        if fill_state() == at {
                            t_fill.card
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
                    .hover(move |s| {
                        if hover_state() == at {
                            s
                        } else {
                            s.bg(t_hover.card.opacity(0.5))
                        }
                    })
            }
            TabsVariant::Underline => {
                let t_rule = t.clone();
                let rule_state = current_for_tab.clone();
                // No hover fill: on the primary rail a filled cell reads as
                // the current tab, and the underline already says which one
                // that is.
                tab.h(px(data::TOP_BAR_HEIGHT - 1.0))
                    .px(px(9.0))
                    .border_b(px(2.0))
                    .border_color(move || {
                        if rule_state() == at {
                            t_rule.primary
                        } else {
                            AdaptiveColor::TRANSPARENT
                        }
                    })
            }
        };
        let tab = tab
            .accessibility_state(move || AccessibilityState {
                selected: current_for_state() == at,
                ..AccessibilityState::default()
            })
            .child(
                text(move || value.clone())
                    .text_size(px(match variant {
                        TabsVariant::Segmented => data::TYPE_BODY,
                        TabsVariant::Underline => data::TYPE_GROUP_NAME,
                    }))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || {
                        if current_for_ink() == at {
                            t.foreground
                        } else {
                            t.muted_foreground
                        }
                    }),
            );
        let tab = match on_change.as_ref() {
            Some(on_change) => {
                let on_change = on_change.clone();
                tab.on_press(move || on_change(at))
            }
            None => tab,
        };
        match item.test_id.clone() {
            Some(id) => tab.test_id(&id),
            None => tab,
        }
    }))
}
