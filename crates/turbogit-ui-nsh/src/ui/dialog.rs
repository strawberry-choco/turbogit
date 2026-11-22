//! The dialog over nsh-base's dialog(): a modal the app styles. 0.5.x's
//! Modal takes no style methods, so the card is a child view wearing the
//! tokens (the n0 spike's finding).

use nsh_base::{dialog as base_dialog, theme};
use nsh_core::prelude::*;

use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};

/// One footer action of a dialog that offers several verbs (a confirm's
/// `Shelve first` beside its `Discard`).
#[derive(Clone)]
pub struct DialogAction {
    pub label: SharedString,
    pub variant: ButtonVariant,
    pub on_press: std::rc::Rc<dyn Fn() + 'static>,
}

impl DialogAction {
    pub fn new(
        label: impl Into<SharedString>,
        variant: ButtonVariant,
        on_press: impl Fn() + 'static,
    ) -> Self {
        DialogAction {
            label: label.into(),
            variant,
            on_press: std::rc::Rc::new(on_press),
        }
    }
}

/// The props of [`dialog`].
pub struct DialogProps {
    /// The dialog's name, the selector's key: `rename-branch`.
    pub name: SharedString,
    /// The title line.
    pub title: SharedString,
    /// The body under the title.
    pub children: Vec<AnyElement>,
    /// The confirm action's label.
    pub action: SharedString,
    /// True styles the action destructive (a discard, a delete).
    pub destructive: bool,
    pub on_action: Option<std::rc::Rc<dyn Fn() + 'static>>,
    pub on_cancel: Option<std::rc::Rc<dyn Fn() + 'static>>,
    /// Extra footer verbs; non-empty replaces the single action, and the
    /// cancel moves last so a confirm reads in prompt order.
    pub actions: Vec<DialogAction>,
    /// A quiet verb above the footer rule (a confirm's disclosure).
    pub disclosure: Option<(SharedString, std::rc::Rc<dyn Fn() + 'static>)>,
    /// The card's width; the default dialog width when unset.
    pub width: Option<f32>,
    /// The primary action cannot be pressed.
    pub action_disabled: bool,
    /// The primary action is withheld: only the cancel remains.
    pub hide_action: bool,
}

impl DialogProps {
    pub fn new(name: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        DialogProps {
            name: name.into(),
            title: title.into(),
            children: Vec::new(),
            action: "Confirm".into(),
            destructive: false,
            on_action: None,
            on_cancel: None,
            actions: Vec::new(),
            disclosure: None,
            width: None,
            action_disabled: false,
            hide_action: false,
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    pub fn action(mut self, action: impl Into<SharedString>) -> Self {
        self.action = action.into();
        self
    }

    pub fn with_action(mut self, action: DialogAction) -> Self {
        self.actions.push(action);
        self
    }

    pub fn disclosure(
        mut self,
        label: impl Into<SharedString>,
        on_press: impl Fn() + 'static,
    ) -> Self {
        self.disclosure = Some((label.into(), std::rc::Rc::new(on_press)));
        self
    }

    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn action_disabled(mut self, disabled: bool) -> Self {
        self.action_disabled = disabled;
        self
    }

    pub fn hide_action(mut self, hide: bool) -> Self {
        self.hide_action = hide;
        self
    }

    pub fn on_action(mut self, on_action: impl Fn() + 'static) -> Self {
        self.on_action = Some(std::rc::Rc::new(on_action));
        self
    }

    pub fn on_cancel(mut self, on_cancel: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(std::rc::Rc::new(on_cancel));
        self
    }
}

/// A dialog: the backdrop takes the outside press, the card floats above
/// it, the footer rule sits above the right-aligned action slot. Escape and
/// the outside press close it through `on_cancel`.
pub fn dialog(props: DialogProps) -> impl IntoElement {
    let t = theme();
    let name = props.name.clone();
    let title = props.title.clone();
    let action_label = props.action.clone();
    let variant = if props.destructive {
        ButtonVariant::Danger
    } else {
        ButtonVariant::Primary
    };
    let cancel = props.on_cancel.clone();
    let confirm = props.on_action.clone();

    let backdrop = match cancel.clone() {
        Some(cancel) => view()
            .absolute()
            .inset_0()
            .bg(t.background.opacity(0.4))
            .on_press(move || cancel()),
        None => view().absolute().inset_0().bg(t.background.opacity(0.4)),
    };

    let mut cancel_button = ButtonProps::new("Cancel").variant(ButtonVariant::Ghost);
    if let Some(cancel) = cancel.clone() {
        cancel_button = cancel_button.on_press(move || cancel());
    }
    let mut footer = view().flex_row().justify_end().gap(px(data::SPACING_X));
    if props.actions.is_empty() {
        let mut action_button = ButtonProps::new(action_label).variant(variant);
        action_button = action_button
            .disabled(props.action_disabled)
            .test_id(&test_ids::dialog_action(&name));
        if let Some(confirm) = confirm.clone() {
            action_button = action_button.on_press(move || confirm());
        }
        if !props.hide_action {
            footer = footer.child(button(action_button));
        }
        footer = footer.child(button(cancel_button));
    } else {
        let last = props.actions.len().saturating_sub(1);
        for (at, action) in props.actions.iter().enumerate() {
            let mut action_button = ButtonProps::new(action.label.clone()).variant(action.variant);
            if at == last {
                action_button = action_button.test_id(&test_ids::dialog_action(&name));
            }
            let on_press = action.on_press.clone();
            action_button = action_button.on_press(move || on_press());
            footer = footer.child(button(action_button));
        }
        footer = footer.child(button(cancel_button));
    }

    let mut card = view()
        .mt(px(96.0))
        .self_center()
        .w(px(props.width.unwrap_or(420.0)))
        .flex_col()
        .gap(px(data::SPACING_Y))
        .rounded(px(data::CARD_RADIUS))
        .border(px(1.0))
        .border_color(t.border)
        .bg(t.card)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::dialog(&name))
        .child(
            text(move || title.clone())
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.card_foreground),
        )
        .children(props.children);
    if let Some((label, run)) = props.disclosure.clone() {
        let mut quiet = ButtonProps::new(label).variant(ButtonVariant::Ghost);
        quiet = quiet.on_press(move || run());
        card = card.child(button(quiet));
    }
    card = card.child(view().h(px(1.0)).bg(t.border)).child(footer);

    base_dialog()
        .close_on_escape(true)
        .close_on_backdrop_press(true)
        .on_cancel(move || {
            if let Some(cancel) = cancel.clone() {
                cancel();
            }
        })
        .child(backdrop)
        .child(card)
}
