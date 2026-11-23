//! The pane header: the one pane chrome row — a title, an optional count
//! chip, a right-aligned action slot, then content. One hairline follows it
//! (the content divider), never two.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;
use crate::ui::chip::{ChipProps, ChipVariant, chip};

/// The props of [`pane_header`].
pub struct PaneHeaderProps {
    /// The title.
    pub title: SharedString,
    /// A trailing count, as a chip.
    pub count: Option<SharedString>,
    /// The count read fresh on every render (a pane header whose count
    /// follows the selection).
    pub count_of: Option<std::rc::Rc<dyn Fn() -> String>>,
    /// The right-aligned action slot.
    pub actions: Vec<AnyElement>,
    pub test_id: Option<SharedString>,
}

impl PaneHeaderProps {
    pub fn new(title: impl Into<SharedString>) -> Self {
        PaneHeaderProps {
            title: title.into(),
            count: None,
            count_of: None,
            actions: Vec::new(),
            test_id: None,
        }
    }

    pub fn count(mut self, count: impl Into<SharedString>) -> Self {
        self.count = Some(count.into());
        self
    }

    pub fn count_of(mut self, count_of: impl Fn() -> String + 'static) -> Self {
        self.count_of = Some(std::rc::Rc::new(count_of));
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_element());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A pane header: title left, count chip and actions right, one hairline
/// under the row.
pub fn pane_header(props: PaneHeaderProps) -> impl IntoElement {
    let t = theme();
    let mut row = view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .child(
            text(move || props.title.clone())
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground)
                .flex_1(),
        );
    if let Some(count) = props.count.clone() {
        row = row.child(chip(ChipProps::new(count).variant(ChipVariant::Raised)));
    }
    if let Some(count_of) = props.count_of.clone() {
        let t = theme();
        row = row.child(
            text(move || SharedString::from(count_of()))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }
    for action in props.actions {
        row = row.child(action);
    }
    let row = row.px(px(data::PANEL_PADDING)).py(px(data::SPACING_Y));
    let mut header = view()
        .flex_col()
        .child(row)
        .child(view().h(px(1.0)).bg(t.border).mx(px(data::PANEL_PADDING)));
    if let Some(id) = props.test_id.clone() {
        header = header.test_id(&id);
    }
    header
}
