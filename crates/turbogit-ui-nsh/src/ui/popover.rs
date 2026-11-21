//! The popover over nsh-base's popover(): a trigger-anchored popup for the
//! pickers that are not menus (the branch picker, the workspace picker).

use nsh_base::popover as base_popover;
use nsh_core::prelude::*;

use crate::theme as data;

/// The props of [`popover`].
pub struct PopoverProps {
    /// The trigger: the control that opens the popover (already styled).
    pub trigger: AnyElement,
    /// The popover's content.
    pub children: Vec<AnyElement>,
    /// The popover's width, when fixed.
    pub width: Option<f32>,
    /// The id UI tests open it by.
    pub test_id: Option<SharedString>,
}

impl PopoverProps {
    pub fn new(trigger: impl IntoElement) -> Self {
        PopoverProps {
            trigger: trigger.into_element(),
            children: Vec::new(),
            width: None,
            test_id: None,
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A popover: the popover surface, the card radius, the panel padding.
/// Escape and the outside press close it, and focus returns to the trigger.
pub fn popover(props: PopoverProps) -> impl IntoElement {
    let mut popup = base_popover()
        .popup_class("rounded-md border border-border p-3 bg-popover shadow-lg")
        .trigger(props.trigger);
    let mut content = view().flex_col().gap(px(data::SPACING_Y));
    if let Some(width) = props.width {
        content = content.w(px(width));
    }
    for child in props.children {
        content = content.child(child);
    }
    popup = popup.child(content);
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(popup),
        None => view().child(popup),
    }
}
