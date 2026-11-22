//! The command over nsh-base's Command: a filter field over grouped rows,
//! the branch picker's list. The search is the control's; the query and the
//! chosen item are reported, never owned here.

use std::rc::Rc;

use nsh_base::{CommandEntry, CommandItem, command as base_command};
use nsh_core::prelude::*;

/// A row's paint.
pub type ItemPaint = Rc<dyn Fn(&CommandEntry) -> AnyElement>;
/// A text handler: the chosen id, or the query after a keystroke.
pub type TextHandler = Rc<dyn Fn(&str)>;

/// The props of [`command`].
pub struct CommandProps {
    /// The items, in order; adjacent items of one group share a heading.
    pub items: Vec<CommandItem>,
    /// The query the app owns, when it owns one.
    pub query: Option<Prop<Option<SharedString>>>,
    /// The query at first.
    pub default_query: Option<SharedString>,
    /// The search field's placeholder.
    pub placeholder: Option<SharedString>,
    pub accessibility_label: Option<SharedString>,
    /// What screen readers call the search field.
    pub search_accessibility_label: Option<SharedString>,
    /// A row's paint.
    pub render_item: Option<ItemPaint>,
    /// The chosen item's id.
    pub on_select: Option<TextHandler>,
    /// The query after a keystroke.
    pub on_query_change: Option<TextHandler>,
    /// Escape with an empty query: close what holds the list.
    pub on_cancel: Option<Rc<dyn Fn()>>,
    pub test_id: Option<SharedString>,
}

impl CommandProps {
    pub fn new(items: Vec<CommandItem>) -> Self {
        CommandProps {
            items,
            query: None,
            default_query: None,
            placeholder: None,
            accessibility_label: None,
            search_accessibility_label: None,
            render_item: None,
            on_select: None,
            on_query_change: None,
            on_cancel: None,
            test_id: None,
        }
    }

    pub fn query(mut self, query: Prop<Option<SharedString>>) -> Self {
        self.query = Some(query);
        self
    }

    pub fn default_query(mut self, query: impl Into<SharedString>) -> Self {
        self.default_query = Some(query.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn search_accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.search_accessibility_label = Some(label.into());
        self
    }

    pub fn render_item(mut self, render: impl Fn(&CommandEntry) -> AnyElement + 'static) -> Self {
        self.render_item = Some(Rc::new(render));
        self
    }

    pub fn on_select(mut self, select: impl Fn(&str) + 'static) -> Self {
        self.on_select = Some(Rc::new(select));
        self
    }

    pub fn on_query_change(mut self, change: impl Fn(&str) + 'static) -> Self {
        self.on_query_change = Some(Rc::new(change));
        self
    }

    pub fn on_cancel(mut self, cancel: impl Fn() + 'static) -> Self {
        self.on_cancel = Some(Rc::new(cancel));
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A command: the search bar, the grouped rows, the highlighted row's band.
pub fn command(props: CommandProps) -> impl IntoElement {
    let mut inner = base_command()
        .items(props.items.clone())
        .searchable(true)
        .filterable(true)
        .auto_focus(true)
        .list_class("max-h-96 overflow-y-auto p-1")
        .item_class("h-7 rounded-sm px-2 hover:bg-muted focus:bg-accent")
        .group_class("px-2 py-1 text-xs")
        .separator_class("h-px bg-border");
    if let Some(query) = props.query {
        inner = inner.query(query);
    }
    if let Some(query) = props.default_query.clone() {
        inner = inner.default_query(Some(query));
    }
    if let Some(placeholder) = props.placeholder.clone() {
        inner = inner.placeholder(placeholder);
    }
    if let Some(label) = props.accessibility_label.clone() {
        inner = inner.accessibility_label(label);
    }
    if let Some(label) = props.search_accessibility_label.clone() {
        inner = inner.search_accessibility_label(label);
    }
    if let Some(render) = props.render_item.clone() {
        inner = inner.render_item(move |entry: &CommandEntry| render(entry));
    }
    if let Some(select) = props.on_select.clone() {
        inner = inner.on_select(move |id: &SharedString| select(id));
    }
    if let Some(change) = props.on_query_change.clone() {
        inner = inner.on_query_change(move |query: &SharedString| change(query));
    }
    if let Some(cancel) = props.on_cancel.clone() {
        inner = inner.on_cancel(move || cancel());
    }
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(inner),
        None => view().child(inner),
    }
}
