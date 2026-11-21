//! The row: one list-row shell and its state grammar. Selection is a
//! composite — the row fill plus the leading rail — and ink does not
//! invert: a selected row's text keeps the color an unselected row's has.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// A row's state: which fill and rail it wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowState {
    /// Resting: transparent.
    #[default]
    Default,
    /// Under the pointer: the shared interaction band.
    Hover,
    /// The list row: the selected fill plus the brand rail at the leading
    /// edge. No list row resolves to the current-ref band; that is a
    /// selection role of its own (the branch kit's).
    Selected,
}

/// The fill each state wears.
fn row_fill(state: RowState) -> AdaptiveColor {
    let t = theme();
    match state {
        RowState::Default => AdaptiveColor::TRANSPARENT,
        RowState::Hover => t.muted,
        RowState::Selected => t.accent,
    }
}

/// The props of [`row`].
pub struct RowProps {
    /// The row's content: its cells, laid out by the caller.
    pub children: Vec<AnyElement>,
    pub state: RowState,
    /// Press target: rows that select on press take it; a fact row (a hash
    /// chip's row) takes none and senses hover only.
    pub on_press: Option<Box<dyn Fn() + 'static>>,
    /// What screen readers say, when the content does not.
    pub accessibility_label: Option<SharedString>,
    pub test_id: Option<SharedString>,
}

impl RowProps {
    pub fn new() -> Self {
        RowProps {
            children: Vec::new(),
            state: RowState::default(),
            on_press: None,
            accessibility_label: None,
            test_id: None,
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    pub fn state(mut self, state: RowState) -> Self {
        self.state = state;
        self
    }

    pub fn on_press(mut self, on_press: impl Fn() + 'static) -> Self {
        self.on_press = Some(Box::new(on_press));
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

impl Default for RowProps {
    fn default() -> Self {
        Self::new()
    }
}

/// A row: the fill from its state, the rail paint on a selected row. The
/// rail is paint, not layout — reserving its width as padding would shift
/// every row's text origin.
pub fn row(props: RowProps) -> impl IntoElement {
    let t = theme();
    let mut content = view()
        .flex_row()
        .items_center()
        .min_h(px(data::FILE_ROW_HEIGHT))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(row_fill(props.state))
        .hover(|s| {
            if props.state == RowState::Default {
                s.bg(t.muted)
            } else {
                s
            }
        });
    for child in props.children {
        content = content.child(child);
    }
    let mut row = view().flex_col().child(content);
    if props.state == RowState::Selected {
        row = row.child(
            view()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(data::RAIL_WIDTH))
                .bg(t.primary),
        );
    }
    if let Some(on_press) = props.on_press {
        row = row.on_press(on_press);
    }
    if let Some(label) = props.accessibility_label.clone() {
        row = row.label(label);
    }
    if let Some(id) = props.test_id.clone() {
        row = row.test_id(&id);
    }
    row
}
