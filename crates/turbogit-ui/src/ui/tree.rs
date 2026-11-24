//! The tree over nsh-base's Tree: the changes list's grouped files, the
//! branch tree. Rows key by the item's id (root and path, root and branch).
//! A row's paint is the app's (`render_item`), and expansion and selection
//! can be the app's own (`expanded`, `selected`).

use std::rc::Rc;

use nsh_base::{SelectValue, TreeEntry, TreeItem, tree as base_tree};
use nsh_core::prelude::*;

/// One entry of a tree.
#[derive(Debug, Clone)]
pub struct TreeEntryProps {
    /// The item's id: unique in the tree (root and path, root and branch).
    pub id: SharedString,
    /// What the row shows.
    pub label: SharedString,
    /// The entries under it; with any, the row expands and collapses.
    pub children: Vec<TreeEntryProps>,
}

impl TreeEntryProps {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        TreeEntryProps {
            id: id.into(),
            label: label.into(),
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: TreeEntryProps) -> Self {
        self.children.push(child);
        self
    }

    fn into_item(self) -> TreeItem {
        let mut item = TreeItem::new(self.id, self.label);
        item.children = self
            .children
            .into_iter()
            .map(TreeEntryProps::into_item)
            .collect();
        item
    }
}

/// A tree's selection handler, by the entry's id.
type SelectHandler = Rc<dyn Fn(&str)>;

/// A row's paint.
pub type ItemPaint = Rc<dyn Fn(&TreeEntry) -> AnyElement>;
/// The expansion the control now holds.
pub type ExpandedHandler = Rc<dyn Fn(&[SharedString])>;

/// The props of [`tree`].
pub struct TreeProps {
    pub items: Vec<TreeEntryProps>,
    /// The expanded ids at first.
    pub default_expanded: Vec<SharedString>,
    /// The expanded ids the app owns.
    pub expanded: Option<Prop<Option<Vec<SharedString>>>>,
    /// The selected id at first.
    pub default_selected: Option<SharedString>,
    /// The selected id the app owns.
    pub selected: Option<Prop<SelectValue>>,
    /// What screen readers call the tree.
    pub accessibility_label: Option<SharedString>,
    pub test_id: Option<SharedString>,
    /// The row's paint, by the entry's own facts.
    pub render_item: Option<ItemPaint>,
    /// The row the user selected, by the entry's id.
    pub on_select: Option<SelectHandler>,
    /// The expanded ids after the user changed them.
    pub on_expanded_change: Option<ExpandedHandler>,
    /// The entry the user acted on (Return, a double press).
    pub on_action: Option<SelectHandler>,
}

impl TreeProps {
    pub fn new(items: Vec<TreeEntryProps>) -> Self {
        TreeProps {
            items,
            default_expanded: Vec::new(),
            expanded: None,
            default_selected: None,
            selected: None,
            accessibility_label: None,
            test_id: None,
            render_item: None,
            on_select: None,
            on_expanded_change: None,
            on_action: None,
        }
    }

    pub fn on_select(mut self, select: impl Fn(&str) + 'static) -> Self {
        self.on_select = Some(Rc::new(select));
        self
    }

    pub fn on_action(mut self, action: impl Fn(&str) + 'static) -> Self {
        self.on_action = Some(Rc::new(action));
        self
    }

    pub fn on_expanded_change(mut self, change: impl Fn(&[SharedString]) + 'static) -> Self {
        self.on_expanded_change = Some(Rc::new(change));
        self
    }

    pub fn expanded(mut self, expanded: Prop<Option<Vec<SharedString>>>) -> Self {
        self.expanded = Some(expanded);
        self
    }

    pub fn selected(mut self, selected: Prop<SelectValue>) -> Self {
        self.selected = Some(selected);
        self
    }

    pub fn render_item(mut self, render: impl Fn(&TreeEntry) -> AnyElement + 'static) -> Self {
        self.render_item = Some(Rc::new(render));
        self
    }

    pub fn default_expanded(mut self, id: impl Into<SharedString>) -> Self {
        self.default_expanded.push(id.into());
        self
    }

    pub fn default_selected(mut self, id: impl Into<SharedString>) -> Self {
        self.default_selected = Some(id.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A tree: rows at the row height the item class carries, the accent band
/// on the selected row, the muted band under the pointer. The tree element
/// takes no test id, so the wrapper names the view that carries it.
pub fn tree(props: TreeProps) -> impl IntoElement {
    let items: Vec<TreeItem> = props
        .items
        .into_iter()
        .map(TreeEntryProps::into_item)
        .collect();
    let mut inner = base_tree()
        .items(items)
        .list_class("p-1")
        .item_class("h-7 flex-row items-center rounded-sm pr-2 hover:bg-muted selected:bg-accent");
    if !props.default_expanded.is_empty() {
        inner = inner.default_expanded(props.default_expanded.clone());
    }
    if let Some(expanded) = props.expanded {
        inner = inner.expanded(expanded);
    }
    if let Some(selected) = props.default_selected.clone() {
        inner = inner.default_selected(selected);
    }
    if let Some(selected) = props.selected {
        inner = inner.selected(selected);
    }
    if let Some(label) = props.accessibility_label.clone() {
        inner = inner.accessibility_label(label);
    }
    if let Some(render) = props.render_item.clone() {
        inner = inner.render_item(move |entry: &TreeEntry| render(entry));
    }
    if let Some(select) = props.on_select.clone() {
        inner = inner.on_selected_change(move |id: &SharedString| select(id));
    }
    if let Some(change) = props.on_expanded_change.clone() {
        inner = inner.on_expanded_change(move |ids: &Vec<SharedString>| change(ids));
    }
    if let Some(action) = props.on_action.clone() {
        inner = inner.on_action(move |id: &SharedString| action(id));
    }
    let host = view().flex_1().child(inner);
    match props.test_id.clone() {
        Some(id) => view().flex_1().test_id(&id).child(host),
        None => view().flex_1().child(host),
    }
}
